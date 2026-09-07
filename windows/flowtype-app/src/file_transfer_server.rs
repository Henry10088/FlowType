use std::collections::HashMap;
use std::convert::Infallible;
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
    tickets: Mutex<HashMap<String, TicketGrant>>,
}

impl FileTransferCoordinator {
    pub(crate) fn load_default() -> std::io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: Arc::new(FileTransferStore::load_default()?),
            tickets: Mutex::new(HashMap::new()),
        }))
    }

    #[cfg(test)]
    fn new(store: FileTransferStore) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(store),
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
    let Some(batch_id) = path
        .strip_prefix("/flowtype/files/v1/batches/")
        .and_then(|value| value.strip_suffix("/manifest"))
        .map(str::to_owned)
    else {
        return Err((StatusCode::NOT_FOUND, "not_found"));
    };
    if request.method() != Method::POST {
        return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
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
    let bearer = bearer_token(&request).ok_or((StatusCode::UNAUTHORIZED, "unauthorized"))?;
    if !coordinator.authorize(&batch_id, bearer) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized"));
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
        .update_state(batch_id, BatchState::Preparing, unix_time())
        .map_err(|_| "batch_state_failed")?;
    Ok(())
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
        let coordinator =
            FileTransferCoordinator::new(FileTransferStore::new(root.clone()).unwrap());
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
