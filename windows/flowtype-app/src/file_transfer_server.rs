use std::collections::HashMap;
use std::convert::Infallible;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use flowtype_core::file_transfer::{
    BatchState, FILE_TRANSFER_VERSION, FileOffer, FileServerMessage, FileTicket, ManifestEntry,
    TransferDirection,
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
}

pub(crate) struct FileTransferCoordinator {
    store: Arc<FileTransferStore>,
    receive_root: PathBuf,
    tickets: Mutex<HashMap<String, TicketGrant>>,
}

impl FileTransferCoordinator {
    pub(crate) fn load_default() -> std::io::Result<Arc<Self>> {
        let receive_root = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "USERPROFILE is unavailable")
            })?
            .join("Downloads")
            .join("FlowType");
        Ok(Arc::new(Self {
            store: Arc::new(FileTransferStore::load_default()?),
            receive_root,
            tickets: Mutex::new(HashMap::new()),
        }))
    }

    #[cfg(test)]
    fn new(store: FileTransferStore, receive_root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(store),
            receive_root,
            tickets: Mutex::new(HashMap::new()),
        })
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
        self.tickets
            .lock()
            .map_err(|_| "ticket service unavailable")?
            .insert(
                batch_id.to_owned(),
                TicketGrant {
                    phone_id: phone_id.to_owned(),
                    token_hash,
                    expires_at_epoch_seconds,
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
        let coordinator = Arc::clone(&state.file_transfers);
        tokio::spawn(async move {
            let _slot = slot;
            let Ok(Ok(stream)) =
                tokio::time::timeout(Duration::from_secs(10), tls.accept(stream)).await
            else {
                return;
            };
            let service =
                service_fn(move |request| serve_request(request, Arc::clone(&coordinator)));
            let _ = http1::Builder::new()
                .keep_alive(true)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

async fn serve_request(
    request: Request<Incoming>,
    coordinator: Arc<FileTransferCoordinator>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let response = match route_request(request, coordinator).await {
        Ok(response) => response,
        Err((status, code)) => json_response(status, format!(r#"{{"code":"{code}"}}"#)),
    };
    Ok(response)
}

async fn route_request(
    request: Request<Incoming>,
    coordinator: Arc<FileTransferCoordinator>,
) -> Result<Response<Full<Bytes>>, (StatusCode, &'static str)> {
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
    let bearer = bearer_token(&request).ok_or((StatusCode::UNAUTHORIZED, "unauthorized"))?;
    if !coordinator.authorize(&batch_id, bearer) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized"));
    }

    if action == "complete" {
        if request.method() != Method::POST {
            return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
        }
        if !coordinator
            .store
            .all_entries_complete(&batch_id)
            .map_err(|_| (StatusCode::CONFLICT, "batch_incomplete"))?
        {
            return Err((StatusCode::CONFLICT, "batch_incomplete"));
        }
        coordinator
            .store
            .update_state(&batch_id, BatchState::Completed, unix_time())
            .map_err(|_| (StatusCode::CONFLICT, "batch_state_failed"))?;
        if let Ok(mut tickets) = coordinator.tickets.lock() {
            tickets.remove(&batch_id);
        }
        return Ok(json_response(
            StatusCode::OK,
            r#"{"completed":true}"#.to_owned(),
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
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim() == "application/x-ndjson")
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
    coordinator
        .store
        .prepare_windows_targets(batch_id, &coordinator.receive_root, unix_time())
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
    fn tagged_offer_round_trips_before_coordinator_validation() {
        let encoded =
            serde_json::to_string(&FileClientMessage::FileOffer(offer("batch-2"))).unwrap();
        let decoded: FileClientMessage = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded, FileClientMessage::FileOffer(_)));
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
        let FileServerMessage::FileTicket(ticket) = message;
        ticket.token
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
