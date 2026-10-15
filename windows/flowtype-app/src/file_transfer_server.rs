use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use flowtype_core::file_transfer::{
    BatchState, EntryKind, FILE_TRANSFER_VERSION, FileDecision, FileOffer, FileServerMessage,
    FileTicket, ManifestEntry, TransferDirection, WindowsNameReservation,
};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{AUTHORIZATION, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use uuid::Uuid;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::Storage::FileSystem::SetFileTime;

use crate::AppState;
use crate::file_transfer_store::EntryProgress;
use crate::file_transfer_store::{BatchRecord, FileTransferStore, ManifestWriter};

pub(crate) const DATA_PORT: u16 = 32188;
const TICKET_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_DATA_CONNECTIONS: usize = 8;

#[derive(Clone)]
struct TicketGrant {
    phone_id: String,
    token_hash: [u8; 32],
    expires_at_epoch_seconds: u64,
    direction: TransferDirection,
}

struct VerificationJob {
    hash: String,
    result: Mutex<Option<Result<(), &'static str>>>,
    started: std::time::Instant,
}

pub(crate) struct FileTransferCoordinator {
    store: Arc<FileTransferStore>,
    receive_root: Mutex<PathBuf>,
    tickets: Mutex<HashMap<String, TicketGrant>>,
    operations: Mutex<HashSet<(String, String)>>,
    verifications: Mutex<HashMap<(String, String), Arc<VerificationJob>>>,
    batch_starts: Mutex<()>,
}

struct OperationLease<'a> {
    coordinator: &'a FileTransferCoordinator,
    key: (String, String),
}

impl Drop for OperationLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut operations) = self.coordinator.operations.lock() {
            operations.remove(&self.key);
        }
        if self
            .coordinator
            .store
            .load_batch(&self.key.0)
            .is_ok_and(|batch| batch.state == BatchState::Canceled)
        {
            cleanup_partial_files(&self.coordinator.store, &self.key.0);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileTransferSnapshot {
    pub(crate) batch_id: String,
    pub(crate) direction: TransferDirection,
    pub(crate) state: BatchState,
    pub(crate) completed_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) completed_entries: u64,
    pub(crate) failed_entries: u64,
    pub(crate) failed_paths: Vec<String>,
    pub(crate) total_entries: u64,
}

impl FileTransferSnapshot {
    pub(crate) fn active(&self) -> bool {
        !self.state.is_terminal()
    }
}

impl FileTransferCoordinator {
    pub(crate) fn handle_control(
        &self,
        value: serde_json::Value,
        phone_id: &str,
    ) -> Option<FileServerMessage> {
        use flowtype_core::file_transfer::FileClientMessage;
        let batch_id = value
            .get("batch_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let result = (|| -> Result<Option<FileServerMessage>, &'static str> {
            match serde_json::from_value::<FileClientMessage>(value)
                .map_err(|_| "invalid file request")?
            {
                FileClientMessage::FileOffer(offer) => {
                    self.accept_offer(&offer, phone_id).map(Some)
                }
                FileClientMessage::FileDecision(decision) => {
                    self.accept_decision(&decision, phone_id)
                }
                FileClientMessage::FileCancel(cancel) => {
                    cancel.validate().map_err(|_| "invalid file cancellation")?;
                    self.cancel(&cancel.batch_id, phone_id)?;
                    Ok(None)
                }
            }
        })();
        result.unwrap_or_else(|code| {
            Some(FileServerMessage::FileError {
                batch_id,
                code: code.to_owned(),
            })
        })
    }

    pub(crate) fn load_default() -> std::io::Result<Arc<Self>> {
        let receive_root = crate::settings::receive_folder()?;
        let store = FileTransferStore::load_default()?;
        cleanup_expired(&store, unix_time().saturating_sub(7 * 24 * 60 * 60));
        Ok(Arc::new(Self {
            store: Arc::new(store),
            receive_root: Mutex::new(receive_root),
            tickets: Mutex::new(HashMap::new()),
            operations: Mutex::new(HashSet::new()),
            verifications: Mutex::new(HashMap::new()),
            batch_starts: Mutex::new(()),
        }))
    }

    #[cfg(test)]
    fn new(store: FileTransferStore, receive_root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(store),
            receive_root: Mutex::new(receive_root),
            tickets: Mutex::new(HashMap::new()),
            operations: Mutex::new(HashSet::new()),
            verifications: Mutex::new(HashMap::new()),
            batch_starts: Mutex::new(()),
        })
    }

    pub(crate) fn receive_root(&self) -> Result<PathBuf, &'static str> {
        self.receive_root
            .lock()
            .map(|path| path.clone())
            .map_err(|_| "receive folder unavailable")
    }

    fn acquire_operation(
        &self,
        batch_id: &str,
        action: &str,
    ) -> Result<OperationLease<'_>, &'static str> {
        // Upload, verification and failure reporting all own the same entry.
        let resource = action.split_once('/').map_or(action, |(_, id)| id);
        let key = (batch_id.to_owned(), resource.to_owned());
        if !action.starts_with("verify/")
            && self
                .verifications
                .lock()
                .map_err(|_| "transfer_busy")?
                .contains_key(&key)
        {
            return Err("transfer_busy");
        }
        if !self
            .operations
            .lock()
            .map_err(|_| "transfer_busy")?
            .insert(key.clone())
        {
            return Err("transfer_busy");
        }
        Ok(OperationLease {
            coordinator: self,
            key,
        })
    }

    fn poll_verification(
        self: &Arc<Self>,
        batch_id: &str,
        entry: &ManifestEntry,
        hash: &str,
        direction: TransferDirection,
    ) -> Result<bool, &'static str> {
        let key = (batch_id.to_owned(), entry.entry_id.clone());
        let mut jobs = self.verifications.lock().map_err(|_| "transfer_busy")?;
        jobs.retain(|_, job| {
            job.started.elapsed() < Duration::from_secs(600)
                || job.result.lock().is_ok_and(|result| result.is_none())
        });
        if let Some(job) = jobs.get(&key) {
            if job.hash != hash {
                return Err("integrity_failed");
            }
            let result = *job.result.lock().map_err(|_| "verification_failed")?;
            if let Some(result) = result {
                jobs.remove(&key);
                return result.map(|()| true);
            }
            return Ok(false);
        }
        if jobs.len() >= MAX_DATA_CONNECTIONS {
            // A disconnected caller may never poll a completed job. Its result
            // can be recomputed from durable progress; only live jobs use slots.
            jobs.retain(|_, job| job.result.lock().is_ok_and(|result| result.is_none()));
        }
        if jobs.len() >= MAX_DATA_CONNECTIONS {
            return Err("transfer_busy");
        }
        let job = Arc::new(VerificationJob {
            hash: hash.to_owned(),
            result: Mutex::new(None),
            started: std::time::Instant::now(),
        });
        jobs.insert(key, Arc::clone(&job));
        let coordinator = Arc::clone(self);
        let batch_id = batch_id.to_owned();
        let entry = entry.clone();
        tokio::task::spawn_blocking(move || {
            let result = verify_entry(&coordinator, &batch_id, &entry, &job.hash, direction);
            if let Ok(mut slot) = job.result.lock() {
                *slot = Some(result);
            }
            if coordinator
                .store
                .load_batch(&batch_id)
                .is_ok_and(|batch| batch.state == BatchState::Canceled)
            {
                cleanup_partial_files(&coordinator.store, &batch_id);
            }
        });
        Ok(false)
    }

    fn prepare_incoming(&self, batch_id: &str) -> Result<(), &'static str> {
        let batch = self
            .store
            .load_batch(batch_id)
            .map_err(|_| "batch_unavailable")?;
        if batch.state.is_terminal() {
            return Err("transfer_canceled");
        }
        let receive_root = batch
            .target_root
            .as_ref()
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| self.receive_root())?;
        if batch.target_root.is_none() && !has_available_space(&receive_root, batch.total_bytes) {
            return Err("insufficient_space");
        }
        self.store
            .prepare_windows_targets(batch_id, &receive_root, unix_time())
            .map_err(|_| "target_preparation_failed")?;
        if matches!(batch.state, BatchState::Queued | BatchState::WaitingNetwork) {
            self.store
                .update_state(batch_id, BatchState::Preparing, unix_time())
                .map_err(|_| "batch_state_failed")?;
        }
        self.store
            .update_state(batch_id, BatchState::Transferring, unix_time())
            .map_err(|_| "batch_state_failed")?;
        Ok(())
    }

    pub(crate) fn set_receive_root(&self, path: PathBuf) -> Result<(), &'static str> {
        *self
            .receive_root
            .lock()
            .map_err(|_| "receive folder unavailable")? = path;
        Ok(())
    }

    pub(crate) fn accept_offer(
        &self,
        offer: &FileOffer,
        authenticated_phone_id: &str,
    ) -> Result<FileServerMessage, &'static str> {
        offer
            .validate(authenticated_phone_id)
            .map_err(|_| "invalid file offer")?;
        if offer.direction != TransferDirection::PhoneToComputer {
            return Err("unexpected transfer direction");
        }
        let _start = self.batch_starts.try_lock().map_err(|_| "transfer_busy")?;
        if self.store.load_batch(&offer.batch_id).is_err()
            && self
                .store
                .recoverable_batches()
                .map_err(|_| "batch_unavailable")?
                .iter()
                .any(|batch| batch.phone_id == authenticated_phone_id)
        {
            return Err("transfer_busy");
        }
        let batch = BatchRecord::new(
            offer.batch_id.clone(),
            offer.phone_id.clone(),
            offer.direction,
            unix_time(),
            offer.file_count,
            offer.directory_count,
            offer.total_bytes,
        )
        .map_err(|_| "invalid batch")?;
        match self.store.create_batch(&batch) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = self
                    .store
                    .load_batch(&offer.batch_id)
                    .map_err(|_| "cannot resume batch")?;
                if existing.phone_id != offer.phone_id
                    || existing.direction != offer.direction
                    || existing.file_count != offer.file_count
                    || existing.directory_count != offer.directory_count
                    || existing.total_bytes != offer.total_bytes
                {
                    return Err("batch id conflict");
                }
                if existing.state.is_terminal() {
                    return Ok(FileServerMessage::FileResult {
                        batch_id: existing.batch_id,
                        state: existing.state,
                    });
                }
            }
            Err(_) => return Err("cannot create batch"),
        }
        self.issue_ticket(&offer.batch_id, authenticated_phone_id)
    }

    pub(crate) fn cancel(&self, batch_id: &str, phone_id: &str) -> Result<(), &'static str> {
        let batch = self
            .store
            .load_batch(batch_id)
            .map_err(|_| "unknown batch")?;
        if batch.phone_id != phone_id {
            return Err("batch owner mismatch");
        }
        if !batch.state.is_terminal() {
            self.store
                .update_state(batch_id, BatchState::Canceled, unix_time())
                .map_err(|_| "cannot cancel batch")?;
        }
        if let Ok(mut tickets) = self.tickets.lock() {
            tickets.remove(batch_id);
        }
        // Also runs after in-flight operations release their handles.
        cleanup_partial_files(&self.store, batch_id);
        Ok(())
    }

    pub(crate) fn revoke_phone(&self, phone_id: &str) {
        if let Ok(mut tickets) = self.tickets.lock() {
            tickets.retain(|_, grant| grant.phone_id != phone_id);
        }
        if let Ok(batches) = self.store.recoverable_batches() {
            for batch in batches
                .into_iter()
                .filter(|batch| batch.phone_id == phone_id)
            {
                let _ = self.cancel(&batch.batch_id, phone_id);
            }
        }
    }

    pub(crate) fn create_outbound_offer(
        &self,
        phone_id: &str,
        roots: &[PathBuf],
    ) -> Result<FileOffer, &'static str> {
        let _start = self.batch_starts.try_lock().map_err(|_| "transfer_busy")?;
        if self
            .store
            .recoverable_batches()
            .map_err(|_| "batch_unavailable")?
            .iter()
            .any(|batch| batch.phone_id == phone_id)
        {
            return Err("a file transfer is already active");
        }
        let prepared = prepare_outbound_plan(roots)?;
        let now = unix_time();
        let batch_id = Uuid::new_v4().to_string();
        let batch = BatchRecord::new(
            batch_id.clone(),
            phone_id.to_owned(),
            TransferDirection::ComputerToPhone,
            now,
            prepared.file_count,
            prepared.directory_count,
            prepared.total_bytes,
        )
        .map_err(|_| "invalid outbound batch")?;
        self.store
            .create_batch(&batch)
            .map_err(|_| "cannot create outbound batch")?;
        let result = (|| {
            let mut manifest = self
                .store
                .begin_manifest(&batch_id)
                .map_err(|_| "cannot create outbound manifest")?;
            let mut sources = self
                .store
                .begin_source_paths(&batch_id)
                .map_err(|_| "cannot create outbound source index")?;
            let mut actual = OutboundStats::default();
            for (path, wire_name) in &prepared.roots {
                write_outbound(path, wire_name, &mut actual, &mut manifest, &mut sources)?;
            }
            if actual.file_count != prepared.file_count
                || actual.directory_count != prepared.directory_count
                || actual.total_bytes != prepared.total_bytes
            {
                return Err("source changed while preparing transfer");
            }
            manifest
                .finish()
                .map_err(|_| "cannot finish outbound manifest")?;
            sources
                .finish()
                .map_err(|_| "cannot save outbound sources")?;
            for ordinal in 0..actual.next_ordinal {
                let entry = self
                    .store
                    .manifest_entry(&batch_id, ordinal)
                    .map_err(|_| "cannot read outbound manifest")?;
                if entry.kind == EntryKind::Directory {
                    self.store
                        .write_progress(
                            &batch_id,
                            entry.ordinal,
                            EntryProgress {
                                state: flowtype_core::file_transfer::EntryState::Completed,
                                committed_offset: 0,
                            },
                        )
                        .map_err(|_| "cannot initialize outbound directory")?;
                }
            }
            Ok(FileOffer {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                transfer_version: FILE_TRANSFER_VERSION,
                phone_id: phone_id.to_owned(),
                batch_id: batch_id.clone(),
                direction: TransferDirection::ComputerToPhone,
                file_count: prepared.file_count,
                directory_count: prepared.directory_count,
                total_bytes: prepared.total_bytes,
                roots: prepared.summaries,
            })
        })();
        if result.is_err() {
            let _ = self.store.delete_batch(&batch_id);
        }
        result
    }

    pub(crate) fn accept_decision(
        &self,
        decision: &FileDecision,
        phone_id: &str,
    ) -> Result<Option<FileServerMessage>, &'static str> {
        decision.validate().map_err(|_| "invalid file decision")?;
        let batch = self
            .store
            .load_batch(&decision.batch_id)
            .map_err(|_| "unknown batch")?;
        if batch.phone_id != phone_id || batch.direction != TransferDirection::ComputerToPhone {
            return Err("batch owner mismatch");
        }
        if batch.state.is_terminal() {
            return Ok(Some(FileServerMessage::FileResult {
                batch_id: batch.batch_id,
                state: batch.state,
            }));
        }
        if !decision.accepted {
            self.cancel(&decision.batch_id, phone_id)?;
            return Ok(None);
        }
        if batch.state == BatchState::WaitingConfirmation {
            self.store
                .update_state(&decision.batch_id, BatchState::Queued, unix_time())
                .map_err(|_| "cannot accept batch")?;
        }
        let state = self
            .store
            .load_batch(&decision.batch_id)
            .map_err(|_| "unknown batch")?
            .state;
        if state == BatchState::Queued || state == BatchState::WaitingNetwork {
            self.store
                .update_state(&decision.batch_id, BatchState::Preparing, unix_time())
                .map_err(|_| "cannot prepare batch")?;
        }
        if self
            .store
            .load_batch(&decision.batch_id)
            .map_err(|_| "unknown batch")?
            .state
            == BatchState::Preparing
        {
            self.store
                .update_state(&decision.batch_id, BatchState::Transferring, unix_time())
                .map_err(|_| "cannot start batch")?;
        }
        Ok(Some(self.issue_ticket(&decision.batch_id, phone_id)?))
    }

    pub(crate) fn pending_outbound_offers(&self, phone_id: &str) -> Vec<FileOffer> {
        self.store
            .recoverable_batches()
            .unwrap_or_default()
            .into_iter()
            .filter(|batch| {
                batch.phone_id == phone_id && batch.direction == TransferDirection::ComputerToPhone
            })
            .filter_map(|batch| {
                let mut roots = Vec::new();
                self.store
                    .for_each_manifest_entry(&batch.batch_id, |entry| {
                        let root = entry.relative_path.split('/').next().unwrap_or_default();
                        if !root.is_empty()
                            && !roots.iter().any(|value| value == root)
                            && roots.len() < 8
                        {
                            roots.push(root.to_owned());
                        }
                        Ok(())
                    })
                    .ok()?;
                Some(FileOffer {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    transfer_version: FILE_TRANSFER_VERSION,
                    phone_id: batch.phone_id,
                    batch_id: batch.batch_id,
                    direction: batch.direction,
                    file_count: batch.file_count,
                    directory_count: batch.directory_count,
                    total_bytes: batch.total_bytes,
                    roots,
                })
            })
            .collect()
    }

    pub(crate) fn transfer_snapshot(&self, phone_id: &str) -> Option<FileTransferSnapshot> {
        let batches = self.store.batches().ok()?;
        let batch = batches
            .into_iter()
            .filter(|batch| batch.phone_id == phone_id)
            .max_by_key(|batch| {
                (
                    !batch.state.is_terminal(),
                    batch.updated_at_epoch_seconds,
                    batch.batch_id.clone(),
                )
            })?;
        let total_entries = batch.file_count.saturating_add(batch.directory_count);
        let progress = self
            .store
            .progress_summary(&batch.batch_id, total_entries)
            .unwrap_or_default();
        let failed_paths = if progress.failed_entries > 0 {
            self.store
                .failed_paths(&batch.batch_id, 5)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Some(FileTransferSnapshot {
            batch_id: batch.batch_id,
            direction: batch.direction,
            state: batch.state,
            completed_bytes: progress.completed_bytes.min(batch.total_bytes),
            total_bytes: batch.total_bytes,
            completed_entries: progress.completed_entries.min(total_entries),
            failed_entries: progress.failed_entries.min(total_entries),
            failed_paths,
            total_entries,
        })
    }

    #[cfg(test)]
    fn cancel_active(&self, phone_id: &str) -> Result<(), &'static str> {
        let batch = self
            .store
            .recoverable_batches()
            .map_err(|_| "file transfer state unavailable")?
            .into_iter()
            .filter(|batch| batch.phone_id == phone_id)
            .max_by_key(|batch| (batch.updated_at_epoch_seconds, batch.batch_id.clone()))
            .ok_or("no active file transfer")?;
        self.cancel(&batch.batch_id, phone_id)
    }

    fn issue_ticket(
        &self,
        batch_id: &str,
        phone_id: &str,
    ) -> Result<FileServerMessage, &'static str> {
        let mut secret = [0u8; 32];
        rand::rng().fill_bytes(&mut secret);
        let token = URL_SAFE_NO_PAD.encode(secret);
        let token_hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let expires_at_epoch_seconds = unix_time().saturating_add(TICKET_LIFETIME.as_secs());
        let direction = self
            .store
            .load_batch(batch_id)
            .map_err(|_| "unknown batch")?
            .direction;
        self.tickets
            .lock()
            .map_err(|_| "ticket service unavailable")?
            .insert(
                batch_id.to_owned(),
                TicketGrant {
                    phone_id: phone_id.to_owned(),
                    token_hash,
                    expires_at_epoch_seconds,
                    direction,
                },
            );
        Ok(FileServerMessage::FileTicket(FileTicket {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            transfer_version: FILE_TRANSFER_VERSION,
            batch_id: batch_id.to_owned(),
            data_port: DATA_PORT,
            token,
            expires_at_epoch_seconds,
        }))
    }

    fn authorize(&self, batch_id: &str, bearer: &str) -> bool {
        let now = unix_time();
        let Ok(mut tickets) = self.tickets.lock() else {
            return false;
        };
        tickets.retain(|_, grant| grant.expires_at_epoch_seconds > now);
        let Some(grant) = tickets.get(batch_id) else {
            return false;
        };
        let supplied: [u8; 32] = Sha256::digest(bearer.as_bytes()).into();
        constant_time_equal(&grant.token_hash, &supplied)
            && self
                .store
                .load_batch(batch_id)
                .is_ok_and(|batch| batch.phone_id == grant.phone_id && !batch.state.is_terminal())
    }

    fn ticket_is_active(&self, batch_id: &str) -> bool {
        let now = unix_time();
        self.tickets.lock().is_ok_and(|tickets| {
            tickets
                .get(batch_id)
                .is_some_and(|grant| grant.expires_at_epoch_seconds > now)
        })
    }

    fn authorized_direction(&self, batch_id: &str, bearer: &str) -> Option<TransferDirection> {
        self.authorize(batch_id, bearer)
            .then(|| {
                self.tickets
                    .lock()
                    .ok()?
                    .get(batch_id)
                    .map(|grant| grant.direction)
            })
            .flatten()
    }
}

fn cleanup_partial_files(store: &FileTransferStore, batch_id: &str) {
    if !store
        .load_batch(batch_id)
        .is_ok_and(|batch| batch.direction == TransferDirection::PhoneToComputer)
        || !store.manifest_is_complete(batch_id)
    {
        return;
    }
    let _ = store.for_each_manifest_entry(batch_id, |entry| {
        if entry.kind == EntryKind::File
            && let Ok(target) = store.target_path(batch_id, entry.ordinal)
            && let Some(parent) = target.parent()
        {
            let _ = fs::remove_file(
                parent.join(format!(".flowtype-{batch_id}-{}.part", entry.entry_id)),
            );
        }
        Ok(())
    });
}

fn cleanup_expired(store: &FileTransferStore, cutoff: u64) {
    let Ok(batches) = store.batches() else { return };
    for batch in batches {
        if batch.updated_at_epoch_seconds >= cutoff {
            continue;
        }
        cleanup_partial_files(store, &batch.batch_id);
        let _ = store.delete_batch(&batch.batch_id);
    }
}

struct OutboundPlan {
    roots: Vec<(PathBuf, String)>,
    summaries: Vec<String>,
    file_count: u64,
    directory_count: u64,
    total_bytes: u64,
}

#[derive(Default)]
struct OutboundStats {
    next_ordinal: u64,
    file_count: u64,
    directory_count: u64,
    total_bytes: u64,
}

fn prepare_outbound_plan(roots: &[PathBuf]) -> Result<OutboundPlan, &'static str> {
    if roots.is_empty() {
        return Err("no files selected");
    }
    let mut root_names = WindowsNameReservation::new(Vec::new());
    let mut planned_roots = Vec::new();
    let mut summaries = Vec::new();
    let mut file_count = 0u64;
    let mut directory_count = 0u64;
    let mut total_bytes = 0u64;
    for root in roots {
        let metadata = fs::symlink_metadata(root).map_err(|_| "source unavailable")?;
        if metadata.file_type().is_symlink() {
            return Err("symbolic links are not supported");
        }
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("source name is not Unicode")?;
        let wire_name = root_names.reserve(name, metadata.is_dir());
        summaries.push(wire_name.clone());
        count_outbound(
            root,
            &mut file_count,
            &mut directory_count,
            &mut total_bytes,
        )?;
        planned_roots.push((root.clone(), wire_name));
    }
    if total_bytes > flowtype_core::file_transfer::MAX_WIRE_INTEGER {
        return Err("batch is too large");
    }
    Ok(OutboundPlan {
        roots: planned_roots,
        summaries: summaries.into_iter().take(8).collect(),
        file_count,
        directory_count,
        total_bytes,
    })
}

fn count_outbound(
    path: &std::path::Path,
    file_count: &mut u64,
    directory_count: &mut u64,
    total_bytes: &mut u64,
) -> Result<(), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "source unavailable")?;
    if metadata.file_type().is_symlink() {
        return Err("symbolic links are not supported");
    }
    if metadata.is_dir() {
        *directory_count = directory_count.checked_add(1).ok_or("batch is too large")?;
        for child in sorted_children(path)? {
            count_outbound(&child.path(), file_count, directory_count, total_bytes)?;
        }
    } else if metadata.is_file() {
        *file_count = file_count.checked_add(1).ok_or("batch is too large")?;
        *total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or("batch is too large")?;
    } else {
        return Err("source type is not supported");
    }
    Ok(())
}

fn write_outbound(
    path: &std::path::Path,
    relative_path: &str,
    stats: &mut OutboundStats,
    manifest: &mut ManifestWriter,
    sources: &mut crate::file_transfer_store::SourceWriter,
) -> Result<(), &'static str> {
    flowtype_core::file_transfer::validate_relative_path(relative_path)
        .map_err(|_| "source path is not transferable")?;
    let metadata = fs::symlink_metadata(path).map_err(|_| "source unavailable")?;
    if metadata.file_type().is_symlink() {
        return Err("symbolic links are not supported");
    }
    let kind = if metadata.is_dir() {
        stats.directory_count = stats
            .directory_count
            .checked_add(1)
            .ok_or("batch is too large")?;
        EntryKind::Directory
    } else {
        stats.file_count = stats
            .file_count
            .checked_add(1)
            .ok_or("batch is too large")?;
        stats.total_bytes = stats
            .total_bytes
            .checked_add(metadata.len())
            .ok_or("batch is too large")?;
        EntryKind::File
    };
    let modified_at_epoch_millis = metadata.modified().ok().and_then(|time| {
        time.duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|value| i64::try_from(value.as_millis()).ok())
    });
    let ordinal = stats.next_ordinal;
    let entry = ManifestEntry {
        ordinal: stats.next_ordinal,
        entry_id: Uuid::new_v4().to_string(),
        relative_path: relative_path.to_owned(),
        kind,
        size: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
        modified_at_epoch_millis,
        mime_type: None,
        sha256: None,
    };
    manifest
        .append(&entry)
        .map_err(|_| "cannot write outbound manifest")?;
    sources
        .append(ordinal, path)
        .map_err(|_| "cannot write outbound source")?;
    stats.next_ordinal = stats
        .next_ordinal
        .checked_add(1)
        .ok_or("batch is too large")?;
    if metadata.is_dir() {
        for child in sorted_children(path)? {
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| "source name is not Unicode")?;
            write_outbound(
                &child.path(),
                &format!("{relative_path}/{name}"),
                stats,
                manifest,
                sources,
            )?;
        }
    }
    Ok(())
}

fn sorted_children(path: &std::path::Path) -> Result<Vec<fs::DirEntry>, &'static str> {
    let mut children = fs::read_dir(path)
        .map_err(|_| "source directory cannot be read")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "source directory cannot be read")?;
    children.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
    Ok(children)
}

pub(crate) async fn run_file_transfer_server(
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, DATA_PORT)).await?;
    let tls = crate::network_server::tls_acceptor(&state.identity)?;
    let slots = Arc::new(Semaphore::new(MAX_DATA_CONNECTIONS));
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
            continue;
        };
        let tls = tls.clone();
        let request_state = Arc::clone(&state);
        tokio::spawn(async move {
            let _slot = slot;
            let Ok(Ok(stream)) =
                tokio::time::timeout(Duration::from_secs(10), tls.accept(stream)).await
            else {
                return;
            };
            let service =
                service_fn(move |request| serve_request(request, Arc::clone(&request_state)));
            let _ = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(15))
                .keep_alive(true)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

async fn serve_request(
    request: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Disk scans and hashes must not occupy workers serving the text socket.
    // The connection semaphore bounds these blocking jobs to eight.
    let runtime = tokio::runtime::Handle::current();
    let request_state = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || {
        runtime.block_on(route_request(request, request_state))
    })
    .await
    .unwrap_or(Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        "file_worker_failed",
    )));
    let response = match result {
        Ok(response) => response,
        Err((status, code)) => json_response(status, format!(r#"{{"code":"{code}"}}"#)),
    };
    state.notify_ui();
    Ok(response)
}

async fn route_request(
    request: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<Full<Bytes>>, (StatusCode, &'static str)> {
    let coordinator = Arc::clone(&state.file_transfers);
    let path = request.uri().path();
    if request.method() == Method::GET && path == "/flowtype/files/v1/health" {
        return Ok(json_response(
            StatusCode::OK,
            r#"{"status":"ok"}"#.to_owned(),
        ));
    }
    let Some(remainder) = path.strip_prefix("/flowtype/files/v1/batches/") else {
        return Err((StatusCode::NOT_FOUND, "not_found"));
    };
    let (batch_id, action) = remainder
        .split_once('/')
        .ok_or((StatusCode::NOT_FOUND, "not_found"))?;
    let batch_id = batch_id.to_owned();
    let direction = authorize_request(&coordinator, &batch_id, bearer_token(&request))?;
    let batch = coordinator
        .store
        .load_batch(&batch_id)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "unauthorized"))?;
    if state.paired_phones.public_key(&batch.phone_id).is_none() {
        coordinator.revoke_phone(&batch.phone_id);
        return Err((StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    let _operation = coordinator
        .acquire_operation(&batch_id, action)
        .map_err(|code| (StatusCode::CONFLICT, code))?;

    if action == "complete" {
        if request.method() != Method::POST {
            return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
        }
        let (all_terminal, has_failures) = coordinator
            .store
            .all_entries_terminal(&batch_id)
            .map_err(|_| (StatusCode::CONFLICT, "batch_incomplete"))?;
        if !all_terminal {
            return Err((StatusCode::CONFLICT, "batch_incomplete"));
        }
        coordinator
            .store
            .update_state(
                &batch_id,
                if has_failures {
                    BatchState::PartialFailure
                } else {
                    BatchState::Completed
                },
                unix_time(),
            )
            .map_err(|_| (StatusCode::CONFLICT, "batch_state_failed"))?;
        if let Ok(mut tickets) = coordinator.tickets.lock() {
            tickets.remove(&batch_id);
        }
        return Ok(json_response(
            StatusCode::OK,
            r#"{"completed":true}"#.to_owned(),
        ));
    }

    if let Some(entry_id) = action.strip_prefix("failures/") {
        if request.method() != Method::POST {
            return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
        }
        let ordinal = request
            .headers()
            .get("x-flowtype-ordinal")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or((StatusCode::BAD_REQUEST, "invalid_ordinal"))?;
        let error_code = request
            .headers()
            .get("x-flowtype-error")
            .and_then(|value| value.to_str().ok())
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 64
                    && value.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
            })
            .ok_or((StatusCode::BAD_REQUEST, "invalid_error"))?;
        let entry = coordinator
            .store
            .manifest_entry(&batch_id, ordinal)
            .map_err(|_| (StatusCode::NOT_FOUND, "entry_not_found"))?;
        if entry.entry_id != entry_id || entry.kind != EntryKind::File {
            return Err((StatusCode::NOT_FOUND, "entry_not_found"));
        }
        let progress = coordinator
            .store
            .read_progress(&batch_id, ordinal)
            .map_err(|_| (StatusCode::CONFLICT, "progress_unavailable"))?;
        // Download verification precedes the phone's SAF commit. The receiver
        // can still fail to publish the file, and is authoritative about that.
        if progress.state != flowtype_core::file_transfer::EntryState::Completed
            || direction == TransferDirection::ComputerToPhone
        {
            coordinator
                .store
                .write_progress(
                    &batch_id,
                    ordinal,
                    EntryProgress {
                        state: flowtype_core::file_transfer::EntryState::Failed,
                        committed_offset: progress.committed_offset,
                    },
                )
                .map_err(|_| (StatusCode::CONFLICT, "progress_unavailable"))?;
            if direction == TransferDirection::PhoneToComputer
                && let Ok(target) = coordinator.store.target_path(&batch_id, ordinal)
                && let Some(parent) = target.parent()
            {
                let _ = fs::remove_file(
                    parent.join(format!(".flowtype-{batch_id}-{}.part", entry.entry_id)),
                );
            }
        }
        return Ok(json_response(
            StatusCode::OK,
            serde_json::json!({"failed": true, "code": error_code}).to_string(),
        ));
    }

    if let Some(entry_id) = action.strip_prefix("verify/") {
        if request.method() != Method::POST {
            return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
        }
        let ordinal = request
            .headers()
            .get("x-flowtype-ordinal")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or((StatusCode::BAD_REQUEST, "invalid_ordinal"))?;
        let supplied_hash = request
            .headers()
            .get("x-flowtype-sha256")
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or((StatusCode::BAD_REQUEST, "invalid_hash"))?;
        let entry = coordinator
            .store
            .manifest_entry(&batch_id, ordinal)
            .map_err(|_| (StatusCode::NOT_FOUND, "entry_not_found"))?;
        if entry.entry_id != entry_id || entry.kind != EntryKind::File {
            return Err((StatusCode::NOT_FOUND, "entry_not_found"));
        }
        if request
            .headers()
            .get("x-flowtype-async-verify")
            .is_some_and(|v| v == "1")
        {
            if !coordinator
                .poll_verification(&batch_id, &entry, supplied_hash, direction)
                .map_err(|code| (StatusCode::CONFLICT, code))?
            {
                return Ok(json_response(
                    StatusCode::ACCEPTED,
                    r#"{"verifying":true}"#.to_owned(),
                ));
            }
        } else {
            verify_entry(&coordinator, &batch_id, &entry, supplied_hash, direction)
                .map_err(|code| (StatusCode::CONFLICT, code))?;
        }
        return Ok(json_response(
            StatusCode::OK,
            serde_json::json!({"verified": true}).to_string(),
        ));
    }

    if let Some(entry_id) = action.strip_prefix("entries/") {
        let ordinal = request
            .headers()
            .get("x-flowtype-ordinal")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or((StatusCode::BAD_REQUEST, "invalid_ordinal"))?;
        let entry = coordinator
            .store
            .manifest_entry(&batch_id, ordinal)
            .map_err(|_| (StatusCode::NOT_FOUND, "entry_not_found"))?;
        if entry.entry_id != entry_id || entry.kind != flowtype_core::file_transfer::EntryKind::File
        {
            return Err((StatusCode::NOT_FOUND, "entry_not_found"));
        }
        if direction == TransferDirection::ComputerToPhone && request.method() == Method::GET {
            let range = request
                .headers()
                .get("range")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| parse_range(value, entry.size))
                .ok_or((StatusCode::RANGE_NOT_SATISFIABLE, "invalid_range"))?;
            let source = coordinator
                .store
                .source_path(&batch_id, ordinal)
                .map_err(|_| (StatusCode::NOT_FOUND, "source_unavailable"))?;
            if !source_matches_entry(&source, &entry) {
                return Err((StatusCode::CONFLICT, "source_changed"));
            }
            let (start, end) = range;
            let length = end - start + 1;
            let mut file = std::fs::File::open(source)
                .map_err(|_| (StatusCode::NOT_FOUND, "source_unavailable"))?;
            file.seek(SeekFrom::Start(start))
                .map_err(|_| (StatusCode::CONFLICT, "source_unavailable"))?;
            let mut bytes = vec![0u8; length as usize];
            file.read_exact(&mut bytes)
                .map_err(|_| (StatusCode::CONFLICT, "source_changed"))?;
            return Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(CONTENT_TYPE, "application/octet-stream")
                .header(
                    "content-range",
                    format!("bytes {start}-{end}/{}", entry.size),
                )
                .header("content-length", length)
                .header("cache-control", "no-store")
                .body(Full::new(Bytes::from(bytes)))
                .expect("valid HTTP response"));
        }
        if direction != TransferDirection::PhoneToComputer {
            return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
        }
        if request.method() == Method::HEAD {
            let progress = coordinator
                .store
                .read_progress(&batch_id, ordinal)
                .map_err(|_| (StatusCode::CONFLICT, "progress_unavailable"))?;
            return Ok(Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header("x-flowtype-offset", progress.committed_offset)
                .header("x-flowtype-length", entry.size)
                .header("cache-control", "no-store")
                .body(Full::new(Bytes::new()))
                .expect("valid HTTP response"));
        }
        if request.method() != Method::POST {
            return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
        }
        let offset = request
            .headers()
            .get("x-flowtype-offset")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or((StatusCode::BAD_REQUEST, "invalid_offset"))?;
        let deferred_hash = request
            .headers()
            .get("x-flowtype-deferred-hash")
            .is_some_and(|value| value == "1");
        let result = receive_file(
            &batch_id,
            &entry,
            offset,
            deferred_hash,
            request.into_body(),
            &coordinator,
            || state.notify_ui(),
        )
        .await
        .map_err(|code| (StatusCode::CONFLICT, code))?;
        return Ok(json_response(
            StatusCode::OK,
            serde_json::json!({
                "committed_offset": result.committed_offset,
                "completed": result.completed,
                "sha256": result.sha256,
            })
            .to_string(),
        ));
    }

    if action != "manifest" {
        return Err((StatusCode::NOT_FOUND, "not_found"));
    }
    if direction == TransferDirection::ComputerToPhone && request.method() == Method::GET {
        let start = request
            .headers()
            .get("x-flowtype-start")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let batch = coordinator
            .store
            .load_batch(&batch_id)
            .map_err(|_| (StatusCode::NOT_FOUND, "batch_not_found"))?;
        let count = batch.file_count.saturating_add(batch.directory_count);
        if start > count {
            return Err((StatusCode::BAD_REQUEST, "invalid_ordinal"));
        }
        let end = start.saturating_add(256).min(count);
        let mut body = Vec::new();
        for ordinal in start..end {
            let entry = coordinator
                .store
                .manifest_entry(&batch_id, ordinal)
                .map_err(|_| (StatusCode::CONFLICT, "manifest_unavailable"))?;
            serde_json::to_writer(&mut body, &entry)
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "manifest_unavailable"))?;
            body.push(b'\n');
        }
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/x-ndjson")
            .header("x-flowtype-next", end)
            .header("x-flowtype-count", count)
            .header("cache-control", "no-store")
            .body(Full::new(Bytes::from(body)))
            .expect("valid HTTP response"));
    }
    if direction != TransferDirection::PhoneToComputer {
        return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
    }
    if request.method() != Method::POST {
        return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
    }
    if coordinator.store.manifest_is_complete(&batch_id) {
        coordinator
            .prepare_incoming(&batch_id)
            .map_err(|code| (StatusCode::CONFLICT, code))?;
        return Ok(json_response(
            StatusCode::OK,
            r#"{"accepted":true,"resumed":true}"#.to_owned(),
        ));
    }
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if content_type
        .split(';')
        .next()
        .is_none_or(|value| value.trim() != "application/x-ndjson")
    {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid_content_type"));
    }
    ingest_manifest(&batch_id, request.into_body(), &coordinator)
        .await
        .map_err(|code| (StatusCode::BAD_REQUEST, code))?;
    Ok(json_response(
        StatusCode::CREATED,
        r#"{"accepted":true}"#.to_owned(),
    ))
}

fn parse_range(value: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let (start, requested_end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    if start >= total {
        return None;
    }
    let requested_end = if requested_end.is_empty() {
        total - 1
    } else {
        requested_end.parse::<u64>().ok()?.min(total - 1)
    };
    if requested_end < start {
        return None;
    }
    let end = requested_end.min(start.saturating_add(8 * 1024 * 1024 - 1));
    Some((start, end))
}

fn authorize_request(
    coordinator: &FileTransferCoordinator,
    batch_id: &str,
    bearer: Option<&str>,
) -> Result<TransferDirection, (StatusCode, &'static str)> {
    let bearer = bearer.ok_or((StatusCode::UNAUTHORIZED, "unauthorized"))?;
    coordinator
        .authorized_direction(batch_id, bearer)
        .ok_or((StatusCode::UNAUTHORIZED, "unauthorized"))
}

async fn ingest_manifest(
    batch_id: &str,
    mut body: Incoming,
    coordinator: &FileTransferCoordinator,
) -> Result<(), &'static str> {
    let mut writer = coordinator
        .store
        .begin_manifest(batch_id)
        .map_err(|_| "manifest_already_started")?;
    let mut pending = Vec::new();
    while let Some(frame) = tokio::time::timeout(Duration::from_secs(30), body.frame())
        .await
        .map_err(|_| "body_idle_timeout")?
    {
        if !coordinator.ticket_is_active(batch_id) {
            return Err("transfer_canceled");
        }
        let frame = frame.map_err(|_| "body_read_failed")?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        pending.extend_from_slice(&data);
        consume_manifest_lines(&mut pending, &mut writer)?;
        if pending.len() > flowtype_core::file_transfer::MAX_MANIFEST_LINE_BYTES {
            return Err("manifest_line_too_large");
        }
    }
    if !pending.is_empty() {
        return Err("incomplete_manifest_line");
    }
    writer.finish().map_err(|_| "manifest_count_mismatch")?;
    coordinator.prepare_incoming(batch_id)
}

fn has_available_space(path: &std::path::Path, required: u64) -> bool {
    if fs::create_dir_all(path).is_err() {
        return false;
    }
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut available = 0u64;
    let queried = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    };
    queried && available >= required.saturating_add(64 * 1024 * 1024)
}

struct ReceiveResult {
    committed_offset: u64,
    completed: bool,
    sha256: Option<String>,
}

async fn receive_file(
    batch_id: &str,
    entry: &ManifestEntry,
    offset: u64,
    deferred_hash: bool,
    mut body: impl hyper::body::Body<Data = Bytes> + Unpin,
    coordinator: &FileTransferCoordinator,
    notify_progress: impl Fn(),
) -> Result<ReceiveResult, &'static str> {
    let ordinal = entry.ordinal;
    if offset > entry.size {
        return Err("invalid_offset");
    }
    let progress = coordinator
        .store
        .read_progress(batch_id, ordinal)
        .map_err(|_| "progress_unavailable")?;
    if offset != progress.committed_offset {
        return Err("offset_mismatch");
    }
    let target = coordinator
        .store
        .target_path(batch_id, ordinal)
        .map_err(|_| "target_unavailable")?;
    if progress.state == flowtype_core::file_transfer::EntryState::Completed {
        let hash = if deferred_hash {
            None
        } else {
            Some(hash_file(&target).map_err(|_| "target_unavailable")?)
        };
        return Ok(ReceiveResult {
            committed_offset: entry.size,
            completed: true,
            sha256: hash,
        });
    }
    let parent = target.parent().ok_or("target_unavailable")?;
    fs::create_dir_all(parent).map_err(|_| "target_unavailable")?;
    let part = parent.join(format!(".flowtype-{batch_id}-{}.part", entry.entry_id));
    if fs::symlink_metadata(&part).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("target_unavailable");
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part)
        .map_err(|_| "target_unavailable")?;
    let staged_len = file.metadata().map_err(|_| "target_unavailable")?.len();
    if staged_len < offset {
        return Err("offset_mismatch");
    }
    if staged_len > offset {
        file.set_len(offset).map_err(|_| "target_unavailable")?;
    }
    let mut hasher = Sha256::new();
    if offset > 0 && !deferred_hash {
        file.seek(SeekFrom::Start(0))
            .map_err(|_| "target_unavailable")?;
        let mut remaining = offset;
        let mut buffer = vec![0u8; 512 * 1024];
        while remaining > 0 {
            if !coordinator.ticket_is_active(batch_id) {
                return Err("transfer_canceled");
            }
            let read_size = remaining.min(buffer.len() as u64) as usize;
            let count = file
                .read(&mut buffer[..read_size])
                .map_err(|_| "target_unavailable")?;
            if count == 0 {
                return Err("offset_mismatch");
            }
            hasher.update(&buffer[..count]);
            remaining -= count as u64;
        }
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| "target_unavailable")?;
    let mut received = offset;
    let mut persisted = offset;
    let mut last_persist = std::time::Instant::now();
    coordinator
        .store
        .write_progress(
            batch_id,
            ordinal,
            EntryProgress {
                state: flowtype_core::file_transfer::EntryState::Transferring,
                committed_offset: offset,
            },
        )
        .map_err(|_| "progress_unavailable")?;
    while let Some(frame) = tokio::time::timeout(Duration::from_secs(30), body.frame())
        .await
        .map_err(|_| "body_idle_timeout")?
    {
        if !coordinator.ticket_is_active(batch_id) {
            // Expiry/re-pairing is not cancellation. Preserve the acknowledged
            // prefix; explicit cancellation cleans it after this request exits.
            return Err("transfer_canceled");
        }
        let frame = frame.map_err(|_| "body_read_failed")?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        received = received
            .checked_add(data.len() as u64)
            .ok_or("size_mismatch")?;
        if received > entry.size {
            return Err("size_mismatch");
        }
        file.write_all(&data).map_err(|_| {
            if has_available_space(parent, entry.size.saturating_sub(received)) {
                "write_failed"
            } else {
                "insufficient_space"
            }
        })?;
        if !deferred_hash {
            hasher.update(&data);
        }
        if received.saturating_sub(persisted) >= 16 * 1024 * 1024
            || last_persist.elapsed() >= Duration::from_secs(1)
        {
            file.sync_data().map_err(|_| "write_failed")?;
            coordinator
                .store
                .write_progress(
                    batch_id,
                    ordinal,
                    EntryProgress {
                        state: flowtype_core::file_transfer::EntryState::Transferring,
                        committed_offset: received,
                    },
                )
                .map_err(|_| "progress_unavailable")?;
            notify_progress();
            persisted = received;
            last_persist = std::time::Instant::now();
        }
    }
    file.flush().map_err(|_| "write_failed")?;
    file.sync_all().map_err(|_| "write_failed")?;
    if received != entry.size {
        coordinator
            .store
            .write_progress(
                batch_id,
                ordinal,
                EntryProgress {
                    state: flowtype_core::file_transfer::EntryState::WaitingNetwork,
                    committed_offset: received,
                },
            )
            .map_err(|_| "progress_unavailable")?;
        return Ok(ReceiveResult {
            committed_offset: received,
            completed: false,
            sha256: None,
        });
    }
    let hash = if deferred_hash {
        None
    } else {
        Some(format!("{:x}", hasher.finalize()))
    };
    if let Some(hash) = &hash
        && entry
            .sha256
            .as_ref()
            .is_some_and(|expected| !expected.eq_ignore_ascii_case(hash))
    {
        return Err("integrity_failed");
    }
    coordinator
        .store
        .write_progress(
            batch_id,
            ordinal,
            EntryProgress {
                state: flowtype_core::file_transfer::EntryState::Transferring,
                committed_offset: received,
            },
        )
        .map_err(|_| "progress_unavailable")?;
    notify_progress();
    Ok(ReceiveResult {
        committed_offset: received,
        completed: false,
        sha256: hash,
    })
}

// A fully received body remains staged until the sender confirms its hash.
// Repeating verification also recovers a crash between rename and persistence.
fn commit_uploaded_file(
    coordinator: &FileTransferCoordinator,
    batch_id: &str,
    entry: &ManifestEntry,
    supplied_hash: &str,
) -> Result<(), &'static str> {
    let target = coordinator
        .store
        .target_path(batch_id, entry.ordinal)
        .map_err(|_| "target_unavailable")?;
    let parent = target.parent().ok_or("target_unavailable")?;
    let part = parent.join(format!(".flowtype-{batch_id}-{}.part", entry.entry_id));
    let staged = part.exists();
    let path = if staged { &part } else { &target };
    if fs::metadata(path).map_err(|_| "target_unavailable")?.len() != entry.size
        || !hash_file(path)
            .map_err(|_| "target_unavailable")?
            .eq_ignore_ascii_case(supplied_hash)
    {
        return Err("integrity_failed");
    }
    if !coordinator.ticket_is_active(batch_id) {
        return Err("transfer_canceled");
    }
    if staged {
        if target.exists() {
            return Err("target_exists");
        }
        fs::rename(&part, &target).map_err(|_| "commit_failed")?;
        if let Some(modified) = entry.modified_at_epoch_millis {
            preserve_modified_time(&target, modified);
        }
    }
    coordinator
        .store
        .write_progress(
            batch_id,
            entry.ordinal,
            EntryProgress {
                state: flowtype_core::file_transfer::EntryState::Completed,
                committed_offset: entry.size,
            },
        )
        .map_err(|_| "progress_unavailable")
}

fn verify_entry(
    coordinator: &FileTransferCoordinator,
    batch_id: &str,
    entry: &ManifestEntry,
    supplied_hash: &str,
    direction: TransferDirection,
) -> Result<(), &'static str> {
    if entry
        .sha256
        .as_ref()
        .is_some_and(|hash| !hash.eq_ignore_ascii_case(supplied_hash))
    {
        return Err("integrity_failed");
    }
    if direction == TransferDirection::PhoneToComputer {
        return commit_uploaded_file(coordinator, batch_id, entry, supplied_hash);
    }
    let source = coordinator
        .store
        .source_path(batch_id, entry.ordinal)
        .map_err(|_| "source_unavailable")?;
    if !source_matches_entry(&source, entry) {
        return Err("source_changed");
    }
    let source_hash = hash_file(&source).map_err(|_| "source_unavailable")?;
    if !source_matches_entry(&source, entry) {
        return Err("source_changed");
    }
    if !source_hash.eq_ignore_ascii_case(supplied_hash) {
        return Err("integrity_failed");
    }
    if !coordinator.ticket_is_active(batch_id) {
        return Err("transfer_canceled");
    }
    coordinator
        .store
        .write_progress(
            batch_id,
            entry.ordinal,
            EntryProgress {
                state: flowtype_core::file_transfer::EntryState::Completed,
                committed_offset: entry.size,
            },
        )
        .map_err(|_| "progress_unavailable")
}

fn hash_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 512 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn preserve_modified_time(path: &std::path::Path, unix_millis: i64) {
    let Ok(unix_millis) = u64::try_from(unix_millis) else {
        return;
    };
    let ticks = unix_millis
        .saturating_add(11_644_473_600_000)
        .saturating_mul(10_000);
    let modified = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    if let Ok(file) = OpenOptions::new().write(true).open(path) {
        unsafe {
            SetFileTime(
                file.as_raw_handle(),
                std::ptr::null(),
                std::ptr::null(),
                &modified,
            );
        }
    }
}

fn consume_manifest_lines(
    pending: &mut Vec<u8>,
    writer: &mut ManifestWriter,
) -> Result<(), &'static str> {
    while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
        if newline > flowtype_core::file_transfer::MAX_MANIFEST_LINE_BYTES {
            return Err("manifest_line_too_large");
        }
        let mut remainder = pending.split_off(newline + 1);
        pending.pop();
        if pending.last() == Some(&b'\r') {
            pending.pop();
        }
        let entry: ManifestEntry =
            serde_json::from_slice(pending).map_err(|_| "invalid_manifest")?;
        writer.append(&entry).map_err(|_| "invalid_manifest")?;
        std::mem::swap(pending, &mut remainder);
    }
    Ok(())
}

fn bearer_token(request: &Request<Incoming>) -> Option<&str> {
    request
        .headers()
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
}

fn json_response(status: StatusCode, body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(body)))
        .expect("valid HTTP response")
}

fn constant_time_equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn source_matches_entry(path: &std::path::Path, entry: &ManifestEntry) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != entry.size {
        return false;
    }
    let Some(expected) = entry.modified_at_epoch_millis else {
        return true;
    };
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_millis()).ok())
        == Some(expected)
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use flowtype_core::file_transfer::{FileClientMessage, FileServerMessage};

    use super::*;

    #[test]
    fn ticket_is_scoped_to_the_batch_and_rotated_on_reoffer() {
        let root = test_root("tickets");
        let receive = root.join("receive");
        let coordinator =
            FileTransferCoordinator::new(FileTransferStore::new(root.clone()).unwrap(), receive);
        let offer = offer("batch-1");
        let first = coordinator.accept_offer(&offer, "phone-1").unwrap();
        let first_token = ticket_token(first);
        assert!(coordinator.authorize("batch-1", &first_token));
        assert!(!coordinator.authorize("batch-2", &first_token));

        let second_token = ticket_token(coordinator.accept_offer(&offer, "phone-1").unwrap());
        assert_ne!(first_token, second_token);
        assert!(!coordinator.authorize("batch-1", &first_token));
        assert!(coordinator.authorize("batch-1", &second_token));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn data_request_rejects_missing_wrong_and_cross_batch_tokens() {
        let root = test_root("request-auth");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );
        let token = ticket_token(
            coordinator
                .accept_offer(&offer("batch-a"), "phone-1")
                .unwrap(),
        );
        let mut second_offer = offer("batch-b");
        second_offer.phone_id = "phone-2".into();
        let other = ticket_token(coordinator.accept_offer(&second_offer, "phone-2").unwrap());

        assert_eq!(
            authorize_request(&coordinator, "batch-a", None),
            Err((StatusCode::UNAUTHORIZED, "unauthorized")),
        );
        assert_eq!(
            authorize_request(&coordinator, "batch-a", Some("wrong")),
            Err((StatusCode::UNAUTHORIZED, "unauthorized")),
        );
        assert_eq!(
            authorize_request(&coordinator, "batch-a", Some(&other)),
            Err((StatusCode::UNAUTHORIZED, "unauthorized")),
        );
        assert_eq!(
            authorize_request(&coordinator, "batch-a", Some(&token)),
            Ok(TransferDirection::PhoneToComputer),
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_ended_download_ranges_are_bounded_to_eight_mibibytes() {
        assert_eq!(
            parse_range("bytes=0-", u64::MAX),
            Some((0, 8 * 1024 * 1024 - 1))
        );
        assert_eq!(
            parse_range("bytes=16777216-", u64::MAX),
            Some((16777216, 16777216 + 8 * 1024 * 1024 - 1)),
        );
        assert_eq!(parse_range("bytes=9-", 10), Some((9, 9)));
    }

    #[test]
    fn storage_preflight_rejects_a_batch_larger_than_the_volume() {
        let root = test_root("insufficient-space");
        assert!(!has_available_space(&root, u64::MAX));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tagged_offer_round_trips_before_coordinator_validation() {
        let encoded =
            serde_json::to_string(&FileClientMessage::FileOffer(offer("batch-2"))).unwrap();
        let decoded: FileClientMessage = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded, FileClientMessage::FileOffer(_)));
    }

    #[test]
    fn incoming_batch_is_visible_and_can_be_canceled_from_the_windows_ui() {
        let root = test_root("incoming-snapshot");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );
        coordinator
            .accept_offer(&offer("batch-incoming"), "phone-1")
            .unwrap();

        let snapshot = coordinator.transfer_snapshot("phone-1").unwrap();
        assert_eq!(snapshot.direction, TransferDirection::PhoneToComputer);
        assert_eq!(snapshot.state, BatchState::Queued);
        assert!(snapshot.active());
        assert_eq!(snapshot.completed_bytes, 0);

        coordinator.cancel_active("phone-1").unwrap();
        let snapshot = coordinator.transfer_snapshot("phone-1").unwrap();
        assert_eq!(snapshot.state, BatchState::Canceled);
        assert!(!snapshot.active());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_receive_root_is_used_by_the_next_incoming_batch() {
        let root = test_root("receive-root");
        let original = root.join("original");
        let changed = root.join("changed");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            original,
        );

        coordinator.set_receive_root(changed.clone()).unwrap();
        assert_eq!(coordinator.receive_root().unwrap(), changed);

        coordinator
            .accept_offer(&offer("batch-new-root"), "phone-1")
            .unwrap();
        assert_eq!(coordinator.receive_root().unwrap(), root.join("changed"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn outbound_directory_is_indexed_and_only_one_batch_is_active() {
        let root = test_root("outbound");
        let input = root.join("input");
        fs::create_dir_all(input.join("子目录")).unwrap();
        fs::write(input.join("a.txt"), b"abc").unwrap();
        fs::write(input.join("子目录").join("b.bin"), [1u8, 2, 3, 4]).unwrap();
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );

        let offer = coordinator
            .create_outbound_offer("phone-1", std::slice::from_ref(&input))
            .unwrap();
        assert_eq!(offer.file_count, 2);
        assert_eq!(offer.directory_count, 2);
        assert_eq!(offer.total_bytes, 7);
        assert_eq!(
            coordinator.store.source_path(&offer.batch_id, 2).unwrap(),
            input.join("子目录")
        );
        assert!(
            coordinator
                .create_outbound_offer("phone-1", std::slice::from_ref(&input))
                .is_err()
        );
        coordinator.cancel_active("phone-1").unwrap();
        assert!(
            coordinator
                .create_outbound_offer("phone-1", &[input])
                .is_ok()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn outbound_source_identity_checks_size_and_modified_time() {
        let root = test_root("source-identity");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.txt");
        fs::write(&source, b"abc").unwrap();
        let metadata = fs::metadata(&source).unwrap();
        let modified = metadata
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let mut entry = ManifestEntry {
            ordinal: 0,
            entry_id: "entry-1".into(),
            relative_path: "source.txt".into(),
            kind: EntryKind::File,
            size: 3,
            modified_at_epoch_millis: Some(modified),
            mime_type: None,
            sha256: None,
        };
        assert!(source_matches_entry(&source, &entry));
        entry.size = 4;
        assert!(!source_matches_entry(&source, &entry));
        entry.size = 3;
        entry.modified_at_epoch_millis = Some(0);
        assert!(!source_matches_entry(&source, &entry));
        fs::remove_dir_all(root).unwrap();
    }

    fn offer(batch_id: &str) -> FileOffer {
        FileOffer {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            transfer_version: FILE_TRANSFER_VERSION,
            phone_id: "phone-1".into(),
            batch_id: batch_id.into(),
            direction: TransferDirection::PhoneToComputer,
            file_count: 1,
            directory_count: 0,
            total_bytes: 42,
            roots: vec!["a.txt".into()],
        }
    }

    #[test]
    fn same_entry_requests_are_exclusive_but_other_entries_can_progress() {
        let root = test_root("operation-leases");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.clone()).unwrap(),
            root.join("received"),
        );
        let lease = coordinator.acquire_operation("batch", "entries/a").unwrap();
        assert!(coordinator.acquire_operation("batch", "verify/a").is_err());
        assert!(
            coordinator
                .acquire_operation("batch", "failures/a")
                .is_err()
        );
        assert!(coordinator.acquire_operation("batch", "entries/b").is_ok());
        drop(lease);
        assert!(coordinator.acquire_operation("batch", "verify/a").is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incoming_and_outgoing_share_one_batch_slot_and_cancel_cleans_late_writers() {
        let root = test_root("batch-slot-cancel");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("received"),
        );
        coordinator
            .accept_offer(&offer("first"), "phone-1")
            .unwrap();
        assert!(
            coordinator
                .accept_offer(&offer("second"), "phone-1")
                .is_err()
        );
        fs::write(root.join("source"), b"source").unwrap();
        assert!(
            coordinator
                .create_outbound_offer("phone-1", &[root.join("source")])
                .is_err()
        );
        // Re-offering this batch is still resumable.
        coordinator
            .accept_offer(&offer("first"), "phone-1")
            .unwrap();
        let mut manifest = coordinator.store.begin_manifest("first").unwrap();
        manifest
            .append(&ManifestEntry {
                ordinal: 0,
                entry_id: "file".into(),
                relative_path: "a.txt".into(),
                kind: EntryKind::File,
                size: 42,
                modified_at_epoch_millis: None,
                mime_type: None,
                sha256: None,
            })
            .unwrap();
        manifest.finish().unwrap();
        coordinator.prepare_incoming("first").unwrap();
        let part = root.join("received/.flowtype-first-file.part");
        let final_file = root.join("received/keep.txt");
        fs::write(&final_file, b"completed").unwrap();
        fs::write(&part, b"partial").unwrap();
        let lease = coordinator
            .acquire_operation("first", "entries/file")
            .unwrap();
        coordinator.cancel("first", "phone-1").unwrap();
        assert!(!part.exists());
        fs::write(&part, b"late writer").unwrap();
        drop(lease);
        assert!(!part.exists());
        assert_eq!(fs::read(final_file).unwrap(), b"completed");
        assert!(
            coordinator
                .accept_offer(&offer("second"), "phone-1")
                .is_ok()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn verification_poll_waits_for_hash_without_holding_a_data_connection() {
        let root = test_root("verify-poll");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("received"),
        );
        fs::write(root.join("source"), b"hello").unwrap();
        let offer = coordinator
            .create_outbound_offer("phone-1", &[root.join("source")])
            .unwrap();
        coordinator
            .accept_decision(
                &FileDecision {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    transfer_version: FILE_TRANSFER_VERSION,
                    batch_id: offer.batch_id.clone(),
                    accepted: true,
                },
                "phone-1",
            )
            .unwrap();
        let entry = coordinator
            .store
            .manifest_entry(&offer.batch_id, 0)
            .unwrap();
        let hash = hash_file(&root.join("source")).unwrap();
        assert!(
            !coordinator
                .poll_verification(&offer.batch_id, &entry, &hash, offer.direction)
                .unwrap()
        );
        assert!(
            coordinator
                .acquire_operation(&offer.batch_id, &format!("entries/{}", entry.entry_id))
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while !coordinator
                .poll_verification(&offer.batch_id, &entry, &hash, offer.direction)
                .unwrap()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            coordinator
                .store
                .all_entries_terminal(&offer.batch_id)
                .unwrap()
                .0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn expired_ticket_preserves_resume_prefix_and_deferred_hash_still_requires_verification()
    {
        let root = test_root("deferred-resume");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("received"),
        );
        coordinator
            .accept_offer(&offer("upload"), "phone-1")
            .unwrap();
        let entry = ManifestEntry {
            ordinal: 0,
            entry_id: "file".into(),
            relative_path: "a.txt".into(),
            kind: EntryKind::File,
            size: 42,
            modified_at_epoch_millis: None,
            mime_type: None,
            sha256: None,
        };
        let mut manifest = coordinator.store.begin_manifest("upload").unwrap();
        manifest.append(&entry).unwrap();
        manifest.finish().unwrap();
        coordinator.prepare_incoming("upload").unwrap();
        let part = root.join("received/.flowtype-upload-file.part");
        fs::write(&part, [b'x'; 3]).unwrap();
        coordinator
            .store
            .write_progress(
                "upload",
                0,
                EntryProgress {
                    state: flowtype_core::file_transfer::EntryState::WaitingNetwork,
                    committed_offset: 3,
                },
            )
            .unwrap();
        coordinator
            .tickets
            .lock()
            .unwrap()
            .get_mut("upload")
            .unwrap()
            .expires_at_epoch_seconds = 0;
        let body = || Full::new(Bytes::from(vec![b'x'; 39]));
        assert!(
            receive_file("upload", &entry, 3, true, body(), &coordinator, || {})
                .await
                .is_err()
        );
        assert_eq!(fs::read(&part).unwrap(), [b'x'; 3]);
        coordinator
            .accept_offer(&offer("upload"), "phone-1")
            .unwrap();
        let result = receive_file("upload", &entry, 3, true, body(), &coordinator, || {})
            .await
            .unwrap();
        assert_eq!(result.committed_offset, 42);
        assert!(!result.completed);
        assert_eq!(result.sha256, None);
        let target = root.join("received/a.txt");
        assert!(!target.exists());
        assert_eq!(
            verify_entry(
                &coordinator,
                "upload",
                &entry,
                &"0".repeat(64),
                TransferDirection::PhoneToComputer
            ),
            Err("integrity_failed")
        );
        let hash = hash_file(&part).unwrap();
        verify_entry(
            &coordinator,
            "upload",
            &entry,
            &hash,
            TransferDirection::PhoneToComputer,
        )
        .unwrap();
        assert_eq!(fs::read(target).unwrap(), [b'x'; 42]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn control_errors_are_replies_and_do_not_poison_subsequent_requests() {
        let root = test_root("control-errors");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );
        let invalid = serde_json::json!({"type":"file_decision","batch_id":"missing",
            "protocol_version":flowtype_core::PROTOCOL_VERSION,
            "transfer_version":FILE_TRANSFER_VERSION,"accepted":true});
        assert!(matches!(coordinator.handle_control(invalid, "phone-1"),
            Some(FileServerMessage::FileError { batch_id, .. }) if batch_id == "missing"));
        let valid = serde_json::to_value(FileClientMessage::FileOffer(offer("next"))).unwrap();
        assert!(matches!(
            coordinator.handle_control(valid, "phone-1"),
            Some(FileServerMessage::FileTicket(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_receipt_survives_lost_response_and_server_restart_in_both_directions() {
        let root = test_root("terminal-receipts");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );
        coordinator
            .accept_offer(&offer("upload"), "phone-1")
            .unwrap();
        for state in [
            BatchState::Preparing,
            BatchState::Transferring,
            BatchState::Completed,
        ] {
            coordinator
                .store
                .update_state("upload", state, unix_time())
                .unwrap();
        }
        fs::write(root.join("source"), b"hello").unwrap();
        let outbound = coordinator
            .create_outbound_offer("phone-1", &[root.join("source")])
            .unwrap();
        let decision = FileDecision {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            transfer_version: FILE_TRANSFER_VERSION,
            batch_id: outbound.batch_id.clone(),
            accepted: true,
        };
        coordinator.accept_decision(&decision, "phone-1").unwrap();
        coordinator
            .store
            .update_state(&outbound.batch_id, BatchState::PartialFailure, unix_time())
            .unwrap();
        drop(coordinator);
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );
        for _ in 0..2 {
            assert!(matches!(
                coordinator
                    .accept_offer(&offer("upload"), "phone-1")
                    .unwrap(),
                FileServerMessage::FileResult {
                    state: BatchState::Completed,
                    ..
                }
            ));
            assert!(matches!(
                coordinator.accept_decision(&decision, "phone-1").unwrap(),
                Some(FileServerMessage::FileResult {
                    state: BatchState::PartialFailure,
                    ..
                })
            ));
        }
        assert!(
            coordinator
                .accept_decision(&decision, "other-phone")
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpair_revokes_existing_tokens_and_in_flight_grants_only_for_that_phone() {
        let root = test_root("unpair");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );
        let first = ticket_token(
            coordinator
                .accept_offer(&offer("first"), "phone-1")
                .unwrap(),
        );
        let mut other = offer("other");
        other.phone_id = "phone-2".into();
        let second = ticket_token(coordinator.accept_offer(&other, "phone-2").unwrap());
        coordinator.revoke_phone("phone-1");
        assert!(!coordinator.authorize("first", &first));
        assert!(!coordinator.ticket_is_active("first"));
        assert_eq!(
            coordinator.store.load_batch("first").unwrap().state,
            BatchState::Canceled
        );
        assert!(coordinator.authorize("other", &second));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uploaded_file_is_committed_only_after_matching_hash_and_commit_is_repeatable() {
        let root = test_root("verified-commit");
        let coordinator = FileTransferCoordinator::new(
            FileTransferStore::new(root.join("state")).unwrap(),
            root.join("receive"),
        );
        coordinator
            .accept_offer(&offer("upload"), "phone-1")
            .unwrap();
        let entry = ManifestEntry {
            ordinal: 0,
            entry_id: "entry-1".into(),
            relative_path: "a.txt".into(),
            kind: EntryKind::File,
            size: 42,
            modified_at_epoch_millis: None,
            mime_type: None,
            sha256: None,
        };
        let mut manifest = coordinator.store.begin_manifest("upload").unwrap();
        manifest.append(&entry).unwrap();
        manifest.finish().unwrap();
        coordinator
            .store
            .prepare_windows_targets("upload", &root.join("receive"), unix_time())
            .unwrap();
        let target = coordinator.store.target_path("upload", 0).unwrap();
        let part = target
            .parent()
            .unwrap()
            .join(".flowtype-upload-entry-1.part");
        fs::write(&part, [b'x'; 42]).unwrap();
        let hash = hash_file(&part).unwrap();
        assert_eq!(
            commit_uploaded_file(&coordinator, "upload", &entry, &"0".repeat(64)),
            Err("integrity_failed")
        );
        assert!(!target.exists());
        assert!(part.exists());
        commit_uploaded_file(&coordinator, "upload", &entry, &hash).unwrap();
        assert!(!part.exists());
        assert_eq!(fs::read(&target).unwrap(), vec![b'x'; 42]);
        // Simulate lost persistence after rename, then replay the verification.
        coordinator
            .store
            .write_progress(
                "upload",
                0,
                EntryProgress {
                    state: flowtype_core::file_transfer::EntryState::Transferring,
                    committed_offset: 42,
                },
            )
            .unwrap();
        commit_uploaded_file(&coordinator, "upload", &entry, &hash).unwrap();
        assert_eq!(
            coordinator.store.read_progress("upload", 0).unwrap().state,
            flowtype_core::file_transfer::EntryState::Completed
        );
        coordinator.revoke_phone("phone-1");
        assert_eq!(
            commit_uploaded_file(&coordinator, "upload", &entry, &hash),
            Err("transfer_canceled")
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn ticket_token(message: FileServerMessage) -> String {
        match message {
            FileServerMessage::FileTicket(ticket) => ticket.token,
            _ => panic!("expected a ticket"),
        }
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "flowtype-file-server-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
