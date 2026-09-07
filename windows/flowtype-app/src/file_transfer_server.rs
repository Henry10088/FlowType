use std::collections::HashMap;
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
use hyper_util::rt::TokioIo;
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

pub(crate) struct FileTransferCoordinator {
    store: Arc<FileTransferStore>,
    receive_root: Mutex<PathBuf>,
    tickets: Mutex<HashMap<String, TicketGrant>>,
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
    pub(crate) fn load_default() -> std::io::Result<Arc<Self>> {
        let receive_root = crate::settings::receive_folder()?;
        let store = FileTransferStore::load_default()?;
        cleanup_expired(&store, unix_time().saturating_sub(7 * 24 * 60 * 60));
        Ok(Arc::new(Self {
            store: Arc::new(store),
            receive_root: Mutex::new(receive_root),
            tickets: Mutex::new(HashMap::new()),
        }))
    }

    #[cfg(test)]
    fn new(store: FileTransferStore, receive_root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(store),
            receive_root: Mutex::new(receive_root),
            tickets: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn receive_root(&self) -> Result<PathBuf, &'static str> {
        self.receive_root
            .lock()
            .map(|path| path.clone())
            .map_err(|_| "receive folder unavailable")
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
                    || existing.state.is_terminal()
                {
                    return Err("batch id conflict");
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
        Ok(())
    }

    pub(crate) fn create_outbound_offer(
        &self,
        phone_id: &str,
        roots: &[PathBuf],
    ) -> Result<FileOffer, &'static str> {
        if self.outbound_batch(phone_id).is_some() {
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

    pub(crate) fn outbound_batch(&self, phone_id: &str) -> Option<BatchRecord> {
        self.store
            .recoverable_batches()
            .ok()?
            .into_iter()
            .filter(|batch| {
                batch.phone_id == phone_id && batch.direction == TransferDirection::ComputerToPhone
            })
            .max_by_key(|batch| (batch.created_at_epoch_seconds, batch.batch_id.clone()))
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

    pub(crate) fn cancel_active(&self, phone_id: &str) -> Result<(), &'static str> {
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

fn cleanup_expired(store: &FileTransferStore, cutoff: u64) {
    let Ok(batches) = store.batches() else { return };
    for batch in batches {
        if batch.updated_at_epoch_seconds >= cutoff {
            continue;
        }
        if store.manifest_is_complete(&batch.batch_id) {
            let _ = store.for_each_manifest_entry(&batch.batch_id, |entry| {
                if batch.direction == TransferDirection::PhoneToComputer
                    && entry.kind == EntryKind::File
                    && let Ok(target) = store.target_path(&batch.batch_id, entry.ordinal)
                    && let Some(parent) = target.parent()
                {
                    let part = parent.join(format!(
                        ".flowtype-{}-{}.part",
                        batch.batch_id, entry.entry_id
                    ));
                    let _ = fs::remove_file(part);
                }
                Ok(())
            });
        }
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
    let response = match route_request(request, Arc::clone(&state)).await {
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
        if progress.state != flowtype_core::file_transfer::EntryState::Completed {
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
        if direction != TransferDirection::ComputerToPhone || request.method() != Method::POST {
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
        let source = coordinator
            .store
            .source_path(&batch_id, ordinal)
            .map_err(|_| (StatusCode::NOT_FOUND, "source_unavailable"))?;
        if !source_matches_entry(&source, &entry) {
            return Err((StatusCode::CONFLICT, "source_changed"));
        }
        let source_hash =
            hash_file(&source).map_err(|_| (StatusCode::CONFLICT, "source_unavailable"))?;
        if !source_hash.eq_ignore_ascii_case(supplied_hash) {
            return Err((StatusCode::CONFLICT, "integrity_failed"));
        }
        coordinator
            .store
            .write_progress(
                &batch_id,
                ordinal,
                EntryProgress {
                    state: flowtype_core::file_transfer::EntryState::Completed,
                    committed_offset: entry.size,
                },
            )
            .map_err(|_| (StatusCode::CONFLICT, "progress_unavailable"))?;
        return Ok(json_response(
            StatusCode::OK,
            serde_json::json!({"verified": true, "sha256": source_hash}).to_string(),
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
        let result = receive_file(
            &batch_id,
            ordinal,
            &entry,
            offset,
            request.into_body(),
            &coordinator,
            &state,
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
    while let Some(frame) = body.frame().await {
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
    let batch = coordinator
        .store
        .load_batch(batch_id)
        .map_err(|_| "batch_unavailable")?;
    let receive_root = coordinator.receive_root()?;
    if !has_available_space(&receive_root, batch.total_bytes) {
        return Err("insufficient_space");
    }
    coordinator
        .store
        .prepare_windows_targets(batch_id, &receive_root, unix_time())
        .map_err(|_| "target_preparation_failed")?;
    coordinator
        .store
        .update_state(batch_id, BatchState::Preparing, unix_time())
        .map_err(|_| "batch_state_failed")?;
    coordinator
        .store
        .update_state(batch_id, BatchState::Transferring, unix_time())
        .map_err(|_| "batch_state_failed")?;
    Ok(())
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
    ordinal: u64,
    entry: &ManifestEntry,
    offset: u64,
    mut body: Incoming,
    coordinator: &FileTransferCoordinator,
    state: &AppState,
) -> Result<ReceiveResult, &'static str> {
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
        let hash = hash_file(&target).map_err(|_| "target_unavailable")?;
        return Ok(ReceiveResult {
            committed_offset: entry.size,
            completed: true,
            sha256: Some(hash),
        });
    }
    let parent = target.parent().ok_or("target_unavailable")?;
    fs::create_dir_all(parent).map_err(|_| "target_unavailable")?;
    let part = parent.join(format!(".flowtype-{batch_id}-{}.part", entry.entry_id));
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
    if offset > 0 {
        file.seek(SeekFrom::Start(0))
            .map_err(|_| "target_unavailable")?;
        let mut remaining = offset;
        let mut buffer = vec![0u8; 512 * 1024];
        while remaining > 0 {
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
    while let Some(frame) = body.frame().await {
        if !coordinator.ticket_is_active(batch_id) {
            drop(file);
            let _ = fs::remove_file(&part);
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
        file.write_all(&data).map_err(|_| "write_failed")?;
        hasher.update(&data);
        if received.saturating_sub(persisted) >= 16 * 1024 * 1024
            || last_persist.elapsed() >= Duration::from_secs(1)
        {
            file.flush().map_err(|_| "write_failed")?;
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
            state.notify_ui();
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
    let hash = format!("{:x}", hasher.finalize());
    if entry
        .sha256
        .as_ref()
        .is_some_and(|expected| !expected.eq_ignore_ascii_case(&hash))
    {
        return Err("integrity_failed");
    }
    drop(file);
    if target.exists() {
        return Err("target_exists");
    }
    fs::rename(&part, &target).map_err(|_| "commit_failed")?;
    if let Some(modified) = entry.modified_at_epoch_millis {
        preserve_modified_time(&target, modified);
    }
    coordinator
        .store
        .write_progress(
            batch_id,
            ordinal,
            EntryProgress {
                state: flowtype_core::file_transfer::EntryState::Completed,
                committed_offset: received,
            },
        )
        .map_err(|_| "progress_unavailable")?;
    state.notify_ui();
    Ok(ReceiveResult {
        committed_offset: received,
        completed: true,
        sha256: Some(hash),
    })
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
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() != entry.size {
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
        let other = ticket_token(
            coordinator
                .accept_offer(&offer("batch-b"), "phone-1")
                .unwrap(),
        );

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

    fn ticket_token(message: FileServerMessage) -> String {
        match message {
            FileServerMessage::FileTicket(ticket) => ticket.token,
            FileServerMessage::FileOffer(_) => panic!("expected a ticket"),
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
