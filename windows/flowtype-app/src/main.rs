#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod identity;
mod injector;
mod network_address;
mod settings;
mod ui;

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use flowtype_core::ipc::{InjectorRequest, InjectorResponse};
use flowtype_core::protocol::{
    Ack, Cancel, ClientMessage, ClientSessionState, ErrorCode, ProbeResult, ProbeState,
    ProtocolError, Resume, ServerMessage, ServerSessionState, SwitchComputer, Target, TargetState,
};
use futures_util::{SinkExt, StreamExt};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use rand::RngCore;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::identity::PcIdentity;
use crate::injector::InjectorClient;

const PORT: u16 = 32187;
const WM_APP_STATE: u32 = 0x8001;

#[derive(Serialize)]
struct PairingPayload<'a> {
    protocol_version: u16,
    pc_id: &'a str,
    pc_name: &'a str,
    candidate_endpoint: String,
    candidate_endpoints: Vec<String>,
    tls_spki_sha256: &'a str,
    one_time_pairing_token: &'a str,
}

#[derive(Deserialize)]
struct AuthMessage {
    protocol_version: u16,
    #[serde(rename = "type")]
    message_type: String,
    phone_id: String,
    phone_name: String,
    #[serde(default)]
    pairing_token: Option<String>,
    #[serde(default)]
    public_key_spki: Option<String>,
    #[serde(default)]
    connection_mode: Option<String>,
    signature: String,
}

#[derive(Serialize)]
struct ChallengeMessage<'a> {
    protocol_version: u16,
    #[serde(rename = "type")]
    message_type: &'static str,
    pc_id: &'a str,
    nonce: &'a str,
}

#[derive(Serialize)]
struct ReadyMessage<'a> {
    protocol_version: u16,
    #[serde(rename = "type")]
    message_type: &'static str,
    pc_id: &'a str,
    pc_name: &'a str,
    candidate_endpoints: Vec<String>,
}

#[derive(Deserialize)]
struct ImageStart {
    protocol_version: u16,
    transfer_id: String,
    phone_id: String,
    mime_type: String,
    width: u32,
    height: u32,
    byte_length: usize,
    sha256: String,
    original: bool,
}

impl ImageStart {
    fn validate(&self, authenticated_phone_id: &str) -> Result<(), &'static str> {
        let max_bytes = if self.original {
            32 * 1024 * 1024
        } else {
            15 * 1024 * 1024
        };
        if self.protocol_version != flowtype_core::PROTOCOL_VERSION
            || self.phone_id != authenticated_phone_id
            || self.transfer_id.is_empty()
            || self.transfer_id.len() > 64
            || !matches!(self.mime_type.as_str(), "image/jpeg" | "image/png")
            || self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > 40_000_000
            || self.byte_length == 0
            || self.byte_length > max_bytes
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|value| value.is_ascii_hexdigit())
        {
            return Err("invalid image metadata");
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ImageReply<'a> {
    protocol_version: u16,
    #[serde(rename = "type")]
    message_type: &'static str,
    transfer_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

#[derive(Clone, Serialize, Deserialize)]
struct PairedPhone {
    phone_name: String,
    public_key_spki: String,
    #[serde(default)]
    paired_at: u64,
    #[serde(default)]
    last_connected: Option<u64>,
}

struct ActiveConnection {
    phone_id: String,
    connection_id: u64,
}

struct OnlineConnection {
    phone_id: String,
    phone_name: String,
    is_probe: bool,
}

#[derive(Clone)]
struct SwitchRequest {
    pc_id: String,
    pc_name: String,
}

struct SwitchChannel {
    sender: UnboundedSender<SwitchRequest>,
    is_control: bool,
}

struct AppState {
    identity: PcIdentity,
    pc_name: Mutex<String>,
    pairing_token: Mutex<Option<String>>,
    paired_phones: Mutex<HashMap<String, PairedPhone>>,
    injector: Mutex<Option<InjectorClient>>,
    active_connection: Mutex<Option<ActiveConnection>>,
    online_connections: Mutex<HashMap<u64, OnlineConnection>>,
    switch_channels: Mutex<HashMap<u64, SwitchChannel>>,
    runtime_status: Mutex<RuntimeStatus>,
    ui_hwnd: AtomicIsize,
    next_connection_id: AtomicU64,
}

#[derive(Clone)]
struct RuntimeStatus {
    summary: String,
    connected_phone: Option<String>,
    target_name: Option<String>,
    last_error: Option<String>,
}

#[derive(Clone)]
struct UiSnapshot {
    pc_name: String,
    phones: Vec<(String, PairedPhone)>,
    status: RuntimeStatus,
    injector_ready: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|argument| argument == "--enable-auto-start") {
        settings::set_auto_start(true)?;
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--disable-auto-start") {
        settings::set_auto_start(false)?;
        return Ok(());
    }
    let Some(_instance) = SingleInstance::acquire()? else {
        ui::show_existing_window();
        return Ok(());
    };
    let identity = PcIdentity::load_or_create()?;
    let paired_phones = load_paired_phones()?;
    let first_run = paired_phones.is_empty();
    let pairing_token = first_run.then(random_token);
    let injector = InjectorClient::connect().ok();
    let endpoint_host = std::env::var("FLOWTYPE_ENDPOINT_HOST")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(network_address::preferred_ipv4);
    let state = Arc::new(AppState {
        identity: identity.clone(),
        pc_name: Mutex::new(identity.pc_name.clone()),
        pairing_token: Mutex::new(pairing_token),
        paired_phones: Mutex::new(paired_phones),
        injector: Mutex::new(injector),
        active_connection: Mutex::new(None),
        online_connections: Mutex::new(HashMap::new()),
        switch_channels: Mutex::new(HashMap::new()),
        runtime_status: Mutex::new(RuntimeStatus {
            summary: "等待手机连接".to_owned(),
            connected_phone: None,
            target_name: None,
            last_error: None,
        }),
        ui_hwnd: AtomicIsize::new(0),
        next_connection_id: AtomicU64::new(1),
    });

    let network_state = Arc::clone(&state);
    let network_error_state = Arc::clone(&state);
    std::thread::Builder::new()
        .name("flowtype-network".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("cannot create network runtime");
            if let Err(error) = runtime.block_on(run_network(network_state, endpoint_host)) {
                eprintln!("network stopped: {error}");
                network_error_state.update_status(|status| {
                    status.summary = "连接服务不可用".to_owned();
                    status.last_error = Some(format!("无法启动局域网连接：{error}"));
                });
            }
        })?;
    let show_requested = std::env::args().any(|argument| argument == "--show");
    ui::run(state, endpoint_host, first_run || show_requested)?;
    Ok(())
}

struct SingleInstance(HANDLE);

impl SingleInstance {
    fn acquire() -> std::io::Result<Option<Self>> {
        let name = wide(r"Local\FlowType.MainApp");
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            Ok(None)
        } else {
            Ok(Some(Self(handle)))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

async fn run_network(
    state: Arc<AppState>,
    endpoint_host: IpAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls = tls_acceptor(&state.identity)?;
    let _mdns = advertise(&state.identity, endpoint_host)?;
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        let tls = tls.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, tls, state).await {
                eprintln!("连接已结束：{error}");
            }
        });
    }
}

impl AppState {
    fn set_ui_hwnd(&self, hwnd: isize) {
        self.ui_hwnd.store(hwnd, Ordering::Release);
    }

    fn snapshot(&self) -> UiSnapshot {
        let mut phones = self
            .paired_phones
            .lock()
            .map(|phones| {
                phones
                    .iter()
                    .map(|(id, phone)| (id.clone(), phone.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        phones.sort_by(|left, right| left.1.phone_name.cmp(&right.1.phone_name));
        UiSnapshot {
            pc_name: self
                .pc_name
                .lock()
                .map(|name| name.clone())
                .unwrap_or_else(|_| self.identity.pc_name.clone()),
            phones,
            status: self
                .runtime_status
                .lock()
                .map(|status| status.clone())
                .unwrap_or_else(|_| RuntimeStatus {
                    summary: "状态不可用".to_owned(),
                    connected_phone: None,
                    target_name: None,
                    last_error: Some("内部状态不可用".to_owned()),
                }),
            injector_ready: self
                .injector
                .lock()
                .map(|injector| injector.is_some())
                .unwrap_or(false),
        }
    }

    fn update_status(&self, update: impl FnOnce(&mut RuntimeStatus)) {
        if let Ok(mut status) = self.runtime_status.lock() {
            update(&mut status);
        }
        self.notify_ui();
    }

    fn notify_ui(&self) {
        let hwnd = self.ui_hwnd.load(Ordering::Acquire);
        if hwnd != 0 {
            unsafe { PostMessageW(hwnd as _, WM_APP_STATE, 0, 0) };
        }
    }

    fn mark_online_connection(&self, phone_id: &str, phone_name: &str, connection_id: u64) {
        if let Ok(mut online) = self.online_connections.lock() {
            online.insert(
                connection_id,
                OnlineConnection {
                    phone_id: phone_id.to_owned(),
                    phone_name: phone_name.to_owned(),
                    is_probe: false,
                },
            );
        }
        self.refresh_online_status();
    }

    fn mark_probe_connection(&self, connection_id: u64) {
        let changed = self
            .online_connections
            .lock()
            .map(|mut online| {
                online
                    .get_mut(&connection_id)
                    .map(|connection| {
                        if connection.is_probe {
                            false
                        } else {
                            connection.is_probe = true;
                            true
                        }
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if changed {
            self.refresh_online_status();
        }
    }

    fn clear_online_connection(&self, connection_id: u64) {
        let removed = self
            .online_connections
            .lock()
            .map(|mut online| online.remove(&connection_id).is_some())
            .unwrap_or(false);
        if removed {
            self.refresh_online_status();
        }
    }

    fn refresh_online_status(&self) {
        let online = self.online_connections.lock().ok().and_then(|connections| {
            connections
                .iter()
                .filter(|(_, connection)| !connection.is_probe)
                .max_by_key(|(connection_id, _)| *connection_id)
                .or_else(|| {
                    connections
                        .iter()
                        .max_by_key(|(connection_id, _)| *connection_id)
                })
                .map(|(_, connection)| (connection.phone_id.clone(), connection.phone_name.clone()))
        });
        self.update_status(|status| {
            if let Some((_, phone_name)) = online {
                status.connected_phone = Some(phone_name.clone());
                status.summary = format!("已连接：{phone_name}");
                status.last_error = None;
            } else {
                status.connected_phone = None;
                status.target_name = None;
                status.summary = "等待手机连接".to_owned();
            }
        });
    }

    fn request_switch_to_current(&self) {
        let pc_name = self
            .pc_name
            .lock()
            .map(|name| name.clone())
            .unwrap_or_else(|_| self.identity.pc_name.clone());
        let request = SwitchRequest {
            pc_id: self.identity.pc_id.clone(),
            pc_name,
        };
        let preferred_id = self
            .active_connection
            .lock()
            .ok()
            .and_then(|active| active.as_ref().map(|connection| connection.connection_id));
        let mut channels = match self.switch_channels.lock() {
            Ok(channels) => channels,
            Err(_) => return,
        };
        let candidate_id = preferred_id
            .filter(|id| channels.contains_key(id))
            .or_else(|| {
                channels
                    .keys()
                    .filter(|id| {
                        channels.get(id).is_some_and(|channel| channel.is_control)
                            || self
                                .online_connections
                                .lock()
                                .ok()
                                .and_then(|online| {
                                    online.get(id).map(|connection| !connection.is_probe)
                                })
                                .unwrap_or(false)
                    })
                    .max()
                    .copied()
            });
        let Some(candidate_id) = candidate_id else {
            return;
        };
        if channels
            .get(&candidate_id)
            .is_some_and(|channel| channel.sender.send(request).is_err())
        {
            channels.remove(&candidate_id);
        }
    }

    fn register_switch_channel(
        &self,
        connection_id: u64,
        sender: UnboundedSender<SwitchRequest>,
        is_control: bool,
    ) {
        if let Ok(mut channels) = self.switch_channels.lock() {
            channels.insert(connection_id, SwitchChannel { sender, is_control });
        }
    }

    fn clear_switch_channel(&self, connection_id: u64) {
        if let Ok(mut channels) = self.switch_channels.lock() {
            channels.remove(&connection_id);
        }
    }

    fn begin_pairing(&self, host: IpAddr) -> Result<String, Box<dyn std::error::Error>> {
        let token = random_token();
        *self
            .pairing_token
            .lock()
            .map_err(|_| "pairing state unavailable")? = Some(token.clone());
        let pc_name = self
            .pc_name
            .lock()
            .map_err(|_| "computer name unavailable")?
            .clone();
        pairing_uri(&self.identity, &pc_name, host, &token).map_err(Into::into)
    }

    fn current_pairing_uri(&self, host: IpAddr) -> Option<String> {
        let token = self.pairing_token.lock().ok()?.clone()?;
        let pc_name = self.pc_name.lock().ok()?.clone();
        pairing_uri(&self.identity, &pc_name, host, &token).ok()
    }

    fn cancel_pairing(&self) {
        if let Ok(mut token) = self.pairing_token.lock() {
            *token = None;
        }
    }

    fn unpair_phone(&self, phone_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut phones = self
            .paired_phones
            .lock()
            .map_err(|_| "phone store unavailable")?;
        phones.remove(phone_id);
        save_paired_phones(&phones)?;
        drop(phones);
        self.notify_ui();
        Ok(())
    }

    fn rename_computer(&self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        PcIdentity::save_pc_name(name)?;
        *self
            .pc_name
            .lock()
            .map_err(|_| "computer name unavailable")? = name.trim().to_owned();
        self.notify_ui();
        Ok(())
    }

    fn mark_input_finished(&self) {
        self.update_status(|status| {
            status.target_name = None;
            status.summary = status
                .connected_phone
                .as_ref()
                .map(|phone| format!("已连接：{phone}"))
                .unwrap_or_else(|| "等待手机连接".to_owned());
        });
    }

    fn repair_injector(&self) -> Result<(), Box<dyn std::error::Error>> {
        let repaired = InjectorClient::repair()?;
        *self
            .injector
            .lock()
            .map_err(|_| "input service state unavailable")? = Some(repaired);
        self.update_status(|status| {
            status.last_error = None;
            if status.connected_phone.is_none() {
                status.summary = "等待手机连接".to_owned();
            }
        });
        Ok(())
    }
}

fn advertise(
    identity: &PcIdentity,
    address: IpAddr,
) -> Result<ServiceDaemon, Box<dyn std::error::Error>> {
    let daemon = ServiceDaemon::new()?;
    let short_id = identity.pc_id.chars().take(8).collect::<String>();
    let host = format!("flowtype-{short_id}.local.");
    let properties = [
        ("pc_id", identity.pc_id.as_str()),
        ("protocol_version", "1"),
    ];
    let service = ServiceInfo::new(
        "_flowtype._tcp.local.",
        &identity.pc_id,
        &host,
        address,
        PORT,
        &properties[..],
    )?;
    daemon.register(service)?;
    Ok(daemon)
}

async fn serve_connection(
    stream: TcpStream,
    tls: TlsAcceptor,
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream = tls.accept(stream).await?;
    let mut websocket = tokio_tungstenite::accept_async(stream).await?;
    let nonce = random_token();
    send_json(
        &mut websocket,
        &ChallengeMessage {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            message_type: "challenge",
            pc_id: &state.identity.pc_id,
            nonce: &nonce,
        },
    )
    .await?;
    let auth = next_text(&mut websocket).await?;
    let auth: AuthMessage = serde_json::from_str(&auth)?;
    if authenticate_phone(&state, &auth, &nonce).is_err() {
        send_json(
            &mut websocket,
            &ServerMessage::Error(ProtocolError {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                code: ErrorCode::AuthFailed,
                session_id: None,
            }),
        )
        .await?;
        return Err("authentication failed".into());
    }
    let connection_id = state.next_connection_id.fetch_add(1, Ordering::Relaxed);
    let pc_name = state
        .pc_name
        .lock()
        .map_err(|_| "computer name unavailable")?
        .clone();
    send_json(
        &mut websocket,
        &ReadyMessage {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            message_type: "ready",
            pc_id: &state.identity.pc_id,
            pc_name: &pc_name,
            candidate_endpoints: endpoint_urls(None),
        },
    )
    .await?;
    let is_control = auth.connection_mode.as_deref() == Some("control");
    let _online_lease = (!is_control).then(|| {
        state.mark_online_connection(&auth.phone_id, &auth.phone_name, connection_id);
        OnlineConnectionLease {
            state: Arc::clone(&state),
            connection_id,
        }
    });
    let (switch_tx, mut switch_rx) = unbounded_channel();
    state.register_switch_channel(connection_id, switch_tx, is_control);
    let _switch_lease = SwitchChannelLease {
        state: Arc::clone(&state),
        connection_id,
    };
    // Authentication is also used by short-lived target probes. Do not claim
    // the single input connection until the client sends a real input message.
    let mut active_lease: Option<ActiveConnectionLease> = None;
    let mut pending_image: Option<ImageStart> = None;
    loop {
        tokio::select! {
            Some(request) = switch_rx.recv() => {
                send_json(
                    &mut websocket,
                    &ServerMessage::SwitchComputer(SwitchComputer {
                        protocol_version: flowtype_core::PROTOCOL_VERSION,
                        pc_id: request.pc_id,
                        pc_name: request.pc_name,
                    }),
                ).await?;
            }
            inbound = websocket.next() => {
                let Some(message) = inbound else { break; };
                match message? {
            Message::Text(text) => {
                if text.len() > flowtype_core::MAX_MESSAGE_BYTES {
                    return Err("message too large".into());
                }
                let value: serde_json::Value = serde_json::from_str(&text)?;
                let is_probe =
                    value.get("type").and_then(serde_json::Value::as_str) == Some("probe");
                if is_probe {
                    state.mark_probe_connection(connection_id);
                }
                if !is_probe && active_lease.is_none() {
                    active_lease = Some(claim_active_connection(
                        &state,
                        &auth.phone_id,
                        &auth.phone_name,
                        connection_id,
                    )?);
                }
                if active_lease.is_some()
                    && !is_active_connection(&state, &auth.phone_id, connection_id)
                {
                    return Err("connection superseded".into());
                }
                if value.get("type").and_then(serde_json::Value::as_str) == Some("image_start") {
                    let image: ImageStart = serde_json::from_value(value)?;
                    image.validate(&auth.phone_id)?;
                    if pending_image.is_some() {
                        send_image_reply(
                            &mut websocket,
                            &image.transfer_id,
                            false,
                            "transfer_busy",
                        )
                        .await?;
                    } else {
                        pending_image = Some(image);
                    }
                } else {
                    let message: ClientMessage = serde_json::from_value(value)?;
                    handle_client_message(&mut websocket, &state, &auth.phone_id, message).await?;
                }
            }
            Message::Binary(bytes) => {
                if active_lease.is_none() {
                    active_lease = Some(claim_active_connection(
                        &state,
                        &auth.phone_id,
                        &auth.phone_name,
                        connection_id,
                    )?);
                }
                if !is_active_connection(&state, &auth.phone_id, connection_id) {
                    return Err("connection superseded".into());
                }
                let Some(image) = pending_image.take() else {
                    continue;
                };
                if bytes.len() != image.byte_length
                    || format!("{:x}", Sha256::digest(&bytes)) != image.sha256.to_ascii_lowercase()
                {
                    send_image_reply(
                        &mut websocket,
                        &image.transfer_id,
                        false,
                        "integrity_failed",
                    )
                    .await?;
                    continue;
                }
                let mime_type = image.mime_type.clone();
                let image_bytes = bytes.to_vec();
                let stored = tokio::task::spawn_blocking(move || {
                    clipboard::set_image(&image_bytes, &mime_type)
                })
                .await
                .map_err(|_| "image worker failed")?;
                if stored.is_ok() {
                    state.update_status(|status| {
                        status.summary = "图片已保存到剪贴板".to_owned();
                        status.last_error = None;
                    });
                    send_image_reply(&mut websocket, &image.transfer_id, true, "").await?;
                } else {
                    state.update_status(|status| {
                        status.last_error = Some("无法写入 Windows 剪贴板".to_owned());
                    });
                    send_image_reply(
                        &mut websocket,
                        &image.transfer_id,
                        false,
                        "clipboard_failed",
                    )
                    .await?;
                }
            }
            Message::Ping(payload) => websocket.send(Message::Pong(payload)).await?,
            Message::Close(_) => break,
            _ => {}
                }
            }
        }
    }
    Ok(())
}

fn authenticate_phone(
    state: &AppState,
    auth: &AuthMessage,
    nonce: &str,
) -> Result<(), &'static str> {
    if auth.protocol_version != flowtype_core::PROTOCOL_VERSION {
        return Err("unsupported protocol");
    }
    let mut phones = state
        .paired_phones
        .lock()
        .map_err(|_| "phone store unavailable")?;
    let public_key = if auth.message_type == "pair" {
        let supplied = auth
            .pairing_token
            .as_deref()
            .ok_or("pairing token required")?;
        let mut token = state
            .pairing_token
            .lock()
            .map_err(|_| "pairing unavailable")?;
        if token.as_deref() != Some(supplied) {
            return Err("invalid pairing token");
        }
        let public_key = auth
            .public_key_spki
            .as_deref()
            .ok_or("public key required")?;
        verify_signature(&state.identity.pc_id, auth, nonce, public_key)?;
        *token = None;
        phones.insert(
            auth.phone_id.clone(),
            PairedPhone {
                phone_name: auth.phone_name.clone(),
                public_key_spki: public_key.to_owned(),
                paired_at: unix_time(),
                last_connected: Some(unix_time()),
            },
        );
        save_paired_phones(&phones).map_err(|_| "cannot save phone")?;
        public_key.to_owned()
    } else if auth.message_type == "authenticate" {
        let public_key = phones
            .get(&auth.phone_id)
            .ok_or("phone is not paired")?
            .public_key_spki
            .clone();
        verify_signature(&state.identity.pc_id, auth, nonce, &public_key)?;
        if let Some(phone) = phones.get_mut(&auth.phone_id) {
            phone.phone_name.clone_from(&auth.phone_name);
            phone.last_connected = Some(unix_time());
        }
        save_paired_phones(&phones).map_err(|_| "cannot save phone")?;
        public_key
    } else {
        return Err("invalid auth type");
    };
    if public_key.is_empty() {
        return Err("public key is empty");
    }
    Ok(())
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn verify_signature(
    pc_id: &str,
    auth: &AuthMessage,
    nonce: &str,
    public_key_spki: &str,
) -> Result<(), &'static str> {
    let public_key = STANDARD
        .decode(public_key_spki)
        .map_err(|_| "invalid public key")?;
    let verifying_key =
        VerifyingKey::from_public_key_der(&public_key).map_err(|_| "invalid public key")?;
    let signature = STANDARD
        .decode(&auth.signature)
        .map_err(|_| "invalid signature")?;
    let signature = Signature::from_der(&signature).map_err(|_| "invalid signature")?;
    verifying_key
        .verify(&auth_payload(pc_id, &auth.phone_id, nonce), &signature)
        .map_err(|_| "signature verification failed")
}

fn auth_payload(pc_id: &str, phone_id: &str, nonce: &str) -> Vec<u8> {
    format!("flowtype-auth-v1\0{pc_id}\0{phone_id}\0{nonce}").into_bytes()
}

fn is_active_connection(state: &AppState, phone_id: &str, connection_id: u64) -> bool {
    state
        .active_connection
        .lock()
        .ok()
        .and_then(|active| {
            active
                .as_ref()
                .map(|active| active.phone_id == phone_id && active.connection_id == connection_id)
        })
        .unwrap_or(false)
}

fn claim_active_connection(
    state: &Arc<AppState>,
    phone_id: &str,
    phone_name: &str,
    connection_id: u64,
) -> Result<ActiveConnectionLease, &'static str> {
    *state
        .active_connection
        .lock()
        .map_err(|_| "connection state unavailable")? = Some(ActiveConnection {
        phone_id: phone_id.to_owned(),
        connection_id,
    });
    state.update_status(|status| {
        status.summary = format!("已连接：{phone_name}");
        status.connected_phone = Some(phone_name.to_owned());
        status.target_name = None;
        status.last_error = None;
    });
    Ok(ActiveConnectionLease {
        state: Arc::clone(state),
        phone_id: phone_id.to_owned(),
        connection_id,
    })
}

struct ActiveConnectionLease {
    state: Arc<AppState>,
    phone_id: String,
    connection_id: u64,
}

fn injector_request(
    state: &AppState,
    request: InjectorRequest,
) -> std::io::Result<InjectorResponse> {
    let mut injector = state
        .injector
        .lock()
        .map_err(|_| std::io::Error::other("input service state unavailable"))?;
    if injector.is_none() {
        *injector = InjectorClient::connect().ok();
    }
    let result = injector
        .as_mut()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "input service unavailable",
            )
        })?
        .request(request);
    if result.is_err() {
        *injector = None;
        state.update_status(|status| {
            status.summary = "Windows 输入服务不可用".to_owned();
            status.last_error = Some("请在设置中修复输入服务".to_owned());
        });
    }
    result
}

impl Drop for ActiveConnectionLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active_connection.lock()
            && active.as_ref().is_some_and(|active| {
                active.phone_id == self.phone_id && active.connection_id == self.connection_id
            })
        {
            *active = None;
            self.state.update_status(|status| {
                status.summary = "等待手机连接".to_owned();
                if let Some(phone) = status.connected_phone.as_deref() {
                    status.summary = format!("已连接：{phone}");
                }
                status.target_name = None;
            });
        }
    }
}

struct SwitchChannelLease {
    state: Arc<AppState>,
    connection_id: u64,
}

struct OnlineConnectionLease {
    state: Arc<AppState>,
    connection_id: u64,
}

impl Drop for OnlineConnectionLease {
    fn drop(&mut self) {
        self.state.clear_online_connection(self.connection_id);
    }
}

impl Drop for SwitchChannelLease {
    fn drop(&mut self) {
        self.state.clear_switch_channel(self.connection_id);
    }
}

async fn handle_client_message<S>(
    websocket: &mut WebSocketStream<S>,
    state: &AppState,
    authenticated_phone_id: &str,
    message: ClientMessage,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    if let ClientMessage::Probe(probe) = &message {
        probe.validate().map_err(|_| "invalid probe")?;
        if probe.phone_id != authenticated_phone_id {
            send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::InvalidMessage,
                    session_id: None,
                }),
            )
            .await?;
            return Ok(());
        }
        let result = match injector_request(state, InjectorRequest::ProbeTarget) {
            Ok(InjectorResponse::TargetReady {
                target_name,
                activity_age_ms,
            }) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Ready,
                target_name: Some(target_name),
                activity_age_ms: Some(activity_age_ms),
            },
            Ok(InjectorResponse::TargetUnsupported) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Unsupported,
                target_name: None,
                activity_age_ms: None,
            },
            Ok(InjectorResponse::TargetInvalid) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Invalid,
                target_name: None,
                activity_age_ms: None,
            },
            Ok(_) | Err(_) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Unsupported,
                target_name: None,
                activity_age_ms: None,
            },
        };
        send_json(websocket, &ServerMessage::ProbeResult(result)).await?;
        return Ok(());
    }

    let (kind, snapshot) = match message {
        ClientMessage::Start(value) => ("start", value),
        ClientMessage::Update(value) => ("update", value),
        ClientMessage::Finish(value) => ("finish", value),
        ClientMessage::Resume(value) => return handle_resume(websocket, state, value).await,
        ClientMessage::Cancel(value) => return handle_cancel(state, value),
        ClientMessage::Probe(_) => unreachable!(),
    };
    snapshot.validate().map_err(|_| "invalid snapshot")?;
    if snapshot.phone_id != authenticated_phone_id {
        return send_json(
            websocket,
            &ServerMessage::Error(ProtocolError {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                code: ErrorCode::InvalidMessage,
                session_id: Some(snapshot.session_id),
            }),
        )
        .await;
    }

    if kind == "start" {
        let response = match injector_request(
            state,
            InjectorRequest::BeginSession {
                session_id: snapshot.session_id.clone(),
            },
        ) {
            Ok(response) => response,
            Err(_) => {
                return send_injector_unavailable(websocket, &snapshot.session_id).await;
            }
        };
        match response {
            InjectorResponse::SessionBegun { target_name } => {
                state.update_status(|status| {
                    status.summary = format!("正在输入到：{target_name}");
                    status.target_name = Some(target_name.clone());
                    status.last_error = None;
                });
                send_json(
                    websocket,
                    &ServerMessage::Target(Target {
                        protocol_version: flowtype_core::PROTOCOL_VERSION,
                        session_id: snapshot.session_id.clone(),
                        target_state: TargetState::Active,
                        target_name: Some(target_name),
                    }),
                )
                .await?;
            }
            other => {
                return send_injector_state(websocket, state, &snapshot.session_id, other).await;
            }
        }
    }

    let applied = match injector_request(
        state,
        InjectorRequest::ApplyState {
            session_id: snapshot.session_id.clone(),
            sequence: snapshot.sequence,
            full_text: snapshot.full_text,
        },
    ) {
        Ok(response) => response,
        Err(_) => return send_injector_unavailable(websocket, &snapshot.session_id).await,
    };
    match applied {
        InjectorResponse::Applied { sequence } if kind == "finish" => {
            let finished = match injector_request(
                state,
                InjectorRequest::FinishSession {
                    session_id: snapshot.session_id.clone(),
                    sequence,
                },
            ) {
                Ok(response) => response,
                Err(_) => return send_injector_unavailable(websocket, &snapshot.session_id).await,
            };
            match finished {
                InjectorResponse::Finished { sequence } => {
                    state.mark_input_finished();
                    send_ack(websocket, &snapshot.session_id, sequence, true).await
                }
                other => send_injector_state(websocket, state, &snapshot.session_id, other).await,
            }
        }
        InjectorResponse::Applied { sequence } => {
            send_ack(websocket, &snapshot.session_id, sequence, false).await
        }
        other => send_injector_state(websocket, state, &snapshot.session_id, other).await,
    }
}

fn handle_cancel(state: &AppState, cancel: Cancel) -> Result<(), Box<dyn std::error::Error>> {
    cancel.validate().map_err(|_| "invalid cancel")?;
    let _ = injector_request(
        state,
        InjectorRequest::CancelInvalidSession {
            session_id: cancel.session_id,
        },
    );
    state.mark_input_finished();
    Ok(())
}

async fn handle_resume<S>(
    websocket: &mut WebSocketStream<S>,
    state: &AppState,
    resume: Resume,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    resume.validate().map_err(|_| "invalid resume")?;
    let applied = match injector_request(
        state,
        InjectorRequest::ApplyState {
            session_id: resume.session_id.clone(),
            sequence: resume.sequence,
            full_text: resume.full_text,
        },
    ) {
        Ok(response) => response,
        Err(_) => return send_injector_unavailable(websocket, &resume.session_id).await,
    };
    match applied {
        InjectorResponse::Applied { sequence }
            if resume.session_state == ClientSessionState::Finishing =>
        {
            let finished = match injector_request(
                state,
                InjectorRequest::FinishSession {
                    session_id: resume.session_id.clone(),
                    sequence,
                },
            ) {
                Ok(response) => response,
                Err(_) => return send_injector_unavailable(websocket, &resume.session_id).await,
            };
            match finished {
                InjectorResponse::Finished { sequence } => {
                    state.mark_input_finished();
                    send_ack(websocket, &resume.session_id, sequence, true).await
                }
                other => send_injector_state(websocket, state, &resume.session_id, other).await,
            }
        }
        InjectorResponse::Applied { sequence } => {
            send_ack(websocket, &resume.session_id, sequence, false).await
        }
        other => send_injector_state(websocket, state, &resume.session_id, other).await,
    }
}

async fn send_ack<S>(
    websocket: &mut WebSocketStream<S>,
    session_id: &str,
    sequence: i64,
    finished: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        websocket,
        &ServerMessage::Ack(Ack {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            session_id: session_id.to_owned(),
            applied_sequence: sequence,
            session_state: if finished {
                ServerSessionState::Finished
            } else {
                ServerSessionState::Active
            },
        }),
    )
    .await
}

async fn send_injector_state<S>(
    websocket: &mut WebSocketStream<S>,
    state: &AppState,
    session_id: &str,
    response: InjectorResponse,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (target_state, target_name) = match response {
        InjectorResponse::TargetNotForeground { target_name } => {
            state.update_status(|status| {
                status.summary = format!("请回到：{target_name}");
                status.target_name = Some(target_name.clone());
            });
            (TargetState::NotForeground, Some(target_name))
        }
        InjectorResponse::TargetInvalid => {
            state.update_status(|status| {
                status.summary = "原输入窗口已关闭".to_owned();
                status.target_name = None;
                status.last_error = Some("请在电脑上重新放置光标，再从手机同步全文".to_owned());
            });
            (TargetState::Invalid, None)
        }
        InjectorResponse::TargetModified => {
            state.update_status(|status| {
                status.summary = "电脑端已编辑".to_owned();
                status.last_error = Some("本次同步已停止，手机正文仍保留".to_owned());
            });
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::TargetModified,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        InjectorResponse::TargetUnsupported => {
            state.update_status(|status| {
                status.summary = "当前应用不支持实时输入".to_owned();
                status.target_name = None;
                status.last_error = Some("请将光标移到其他输入框后重试".to_owned());
            });
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::TargetUnavailable,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        InjectorResponse::InjectionUnknown | InjectorResponse::InvalidRequest => {
            let code = if response == InjectorResponse::InjectionUnknown {
                ErrorCode::InjectionUnknown
            } else {
                ErrorCode::SequenceConflict
            };
            state.update_status(|status| {
                status.summary = "输入已停止".to_owned();
                status.last_error = Some(if code == ErrorCode::InjectionUnknown {
                    "Windows 无法确认本次输入结果".to_owned()
                } else {
                    "输入状态不一致".to_owned()
                });
            });
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        _ => (TargetState::Invalid, None),
    };
    send_json(
        websocket,
        &ServerMessage::Target(Target {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            session_id: session_id.to_owned(),
            target_state,
            target_name,
        }),
    )
    .await
}

async fn send_injector_unavailable<S>(
    websocket: &mut WebSocketStream<S>,
    session_id: &str,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        websocket,
        &ServerMessage::Error(ProtocolError {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            code: ErrorCode::InjectorUnavailable,
            session_id: Some(session_id.to_owned()),
        }),
    )
    .await
}

async fn send_json<S, T>(
    websocket: &mut WebSocketStream<S>,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    T: Serialize,
{
    websocket
        .send(Message::Text(serde_json::to_string(value)?.into()))
        .await?;
    Ok(())
}

async fn send_image_reply<S>(
    websocket: &mut WebSocketStream<S>,
    transfer_id: &str,
    success: bool,
    error_code: &'static str,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        websocket,
        &ImageReply {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            message_type: if success { "image_ack" } else { "image_error" },
            transfer_id,
            code: (!success).then_some(error_code),
        },
    )
    .await
}

async fn next_text<S>(
    websocket: &mut WebSocketStream<S>,
) -> Result<String, Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match websocket.next().await.ok_or("connection closed")?? {
        Message::Text(value) => Ok(value.to_string()),
        _ => Err("expected text message".into()),
    }
}

fn tls_acceptor(identity: &PcIdentity) -> Result<TlsAcceptor, Box<dyn std::error::Error>> {
    let provider = rustls::crypto::ring::default_provider();
    let config = ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(identity.cert_der.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone())),
        )?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pairing_uri(
    identity: &PcIdentity,
    pc_name: &str,
    host: IpAddr,
    token: &str,
) -> Result<String, serde_json::Error> {
    let primary_endpoint = format!("wss://{host}:{PORT}/v1/sync");
    let candidate_endpoints = endpoint_urls(Some(&primary_endpoint));
    let payload = PairingPayload {
        protocol_version: flowtype_core::PROTOCOL_VERSION,
        pc_id: &identity.pc_id,
        pc_name,
        candidate_endpoint: primary_endpoint,
        candidate_endpoints,
        tls_spki_sha256: &identity.spki_sha256,
        one_time_pairing_token: token,
    };
    Ok(format!(
        "flowtype://pair?data={}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?)
    ))
}

fn endpoint_urls(primary: Option<&str>) -> Vec<String> {
    let mut endpoints = primary.into_iter().map(str::to_owned).collect::<Vec<_>>();
    endpoints.extend(
        network_address::candidate_ipv4s()
            .into_iter()
            .map(|address| format!("wss://{address}:{PORT}/v1/sync")),
    );
    endpoints.dedup();
    endpoints
}

fn phones_path() -> Result<std::path::PathBuf, std::io::Error> {
    Ok(identity::data_dir()?.join("paired-phones-v2.json"))
}

fn load_paired_phones() -> Result<HashMap<String, PairedPhone>, Box<dyn std::error::Error>> {
    let path = phones_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn save_paired_phones(
    phones: &HashMap<String, PairedPhone>,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(phones_path()?, serde_json::to_vec(phones)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{Signature, SigningKey};
    use p256::pkcs8::EncodePublicKey;

    use super::{AuthMessage, ImageStart, auth_payload, verify_signature};

    #[test]
    fn verifies_a_phone_challenge_signature() {
        let signing_key = SigningKey::from_bytes((&[7_u8; 32]).into()).unwrap();
        let payload = auth_payload("pc", "phone", "nonce");
        let signature: Signature = signing_key.sign(&payload);
        let auth = AuthMessage {
            protocol_version: 1,
            message_type: "authenticate".to_owned(),
            phone_id: "phone".to_owned(),
            phone_name: "test".to_owned(),
            pairing_token: None,
            public_key_spki: None,
            connection_mode: None,
            signature: STANDARD.encode(signature.to_der()),
        };
        let point = signing_key.verifying_key().to_encoded_point(false);
        let public = p256::PublicKey::from_sec1_bytes(point.as_bytes()).unwrap();
        let public_key = STANDARD.encode(public.to_public_key_der().unwrap().as_bytes());

        assert!(verify_signature("pc", &auth, "nonce", &public_key).is_ok());
        assert!(verify_signature("pc", &auth, "different", &public_key).is_err());
    }

    #[test]
    fn validates_image_transfer_limits_and_phone() {
        let image = ImageStart {
            protocol_version: 1,
            transfer_id: "transfer-1".to_owned(),
            phone_id: "phone".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 100,
            height: 100,
            byte_length: 1024,
            sha256: "a".repeat(64),
            original: false,
        };
        assert!(image.validate("phone").is_ok());
        assert!(image.validate("different-phone").is_err());

        let oversized = ImageStart {
            byte_length: 16 * 1024 * 1024,
            ..image
        };
        assert!(oversized.validate("phone").is_err());
    }
}
