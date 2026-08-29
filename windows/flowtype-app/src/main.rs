#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod atomic_file;
mod clipboard;
mod diagnostics;
mod i18n;
mod identity;
mod injector;
mod injector_dispatcher;
mod network_address;
mod network_server;
mod pairing_store;
mod settings;
mod ui;
mod update;

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use flowtype_core::ipc::{InjectorRequest, InjectorResponse};
use flowtype_core::protocol::{
    Ack, Cancel, ClientMessage, ClientSessionState, ErrorCode, HealthAck, ProbeResult, ProbeState,
    ProtocolError, Resume, ServerMessage, ServerSessionState, SwitchAck, SwitchComputer, Target,
    TargetState,
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
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{Sender, channel};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::i18n::tr;
use crate::identity::PcIdentity;
use crate::injector_dispatcher::{InjectorDispatcher, InjectorRequestFailure};
#[cfg(test)]
use crate::injector_dispatcher::{
    ReconcileDecision, classify_apply_recovery, classify_finish_recovery,
};
use crate::network_server::run_network;
#[cfg(test)]
use crate::network_server::{
    auth_payload, validate_cancel_for_phone, validate_resume_for_phone, verify_signature,
};
use crate::pairing_store::{PairedPhone, PairedPhoneStore};
#[cfg(test)]
use crate::pairing_store::{deduplicate_paired_phones, upsert_paired_phone};

const PORT: u16 = 32187;
const WM_APP_STATE: u32 = 0x8001;
const MAX_CONNECTIONS: usize = 32;
const MAX_CONNECTIONS_PER_IP: usize = 8;
const MAX_CONNECTION_ATTEMPTS_PER_MINUTE: usize = 30;
const MAX_PEER_LIMIT_ENTRIES: usize = 1024;
const MAX_AUTH_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECTION_RATE_WINDOW: Duration = Duration::from_secs(60);

struct PeerConnectionState {
    active: usize,
    attempts: VecDeque<Instant>,
    last_seen: Instant,
}

struct ConnectionLimiter {
    peers: Mutex<HashMap<IpAddr, PeerConnectionState>>,
}

struct PeerConnectionLease {
    limiter: Arc<ConnectionLimiter>,
    peer: IpAddr,
}

impl ConnectionLimiter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            peers: Mutex::new(HashMap::new()),
        })
    }

    fn try_acquire(self: &Arc<Self>, peer: IpAddr) -> Option<PeerConnectionLease> {
        let now = Instant::now();
        let mut peers = self.peers.lock().ok()?;
        if peers.len() >= MAX_PEER_LIMIT_ENTRIES {
            peers.retain(|_, state| {
                state.active > 0
                    || now.saturating_duration_since(state.last_seen) < CONNECTION_RATE_WINDOW
            });
            if peers.len() >= MAX_PEER_LIMIT_ENTRIES && !peers.contains_key(&peer) {
                return None;
            }
        }
        let state = peers.entry(peer).or_insert_with(|| PeerConnectionState {
            active: 0,
            attempts: VecDeque::new(),
            last_seen: now,
        });
        while state.attempts.front().is_some_and(|attempt| {
            now.saturating_duration_since(*attempt) >= CONNECTION_RATE_WINDOW
        }) {
            state.attempts.pop_front();
        }
        state.last_seen = now;
        if state.active >= MAX_CONNECTIONS_PER_IP
            || state.attempts.len() >= MAX_CONNECTION_ATTEMPTS_PER_MINUTE
        {
            return None;
        }
        state.active += 1;
        state.attempts.push_back(now);
        Some(PeerConnectionLease {
            limiter: Arc::clone(self),
            peer,
        })
    }
}

impl Drop for PeerConnectionLease {
    fn drop(&mut self) {
        if let Ok(mut peers) = self.limiter.peers.lock()
            && let Some(state) = peers.get_mut(&self.peer)
        {
            state.active = state.active.saturating_sub(1);
        }
    }
}

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

#[derive(Clone, Deserialize)]
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
    #[serde(default)]
    capabilities: Vec<String>,
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
    capabilities: &'static [&'static str],
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
            MAX_IMAGE_BYTES
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
    request_id: String,
}

struct PendingSwitch {
    pc_id: String,
    request_id: String,
}

struct SwitchChannel {
    sender: Sender<SwitchRequest>,
    is_control: bool,
    supports_switch_ack: bool,
}

struct AppState {
    identity: PcIdentity,
    pc_name: Mutex<String>,
    pairing_token: Mutex<Option<String>>,
    paired_phones: PairedPhoneStore,
    pairing_slot: Arc<Semaphore>,
    injector: InjectorDispatcher,
    active_connection: Mutex<Option<ActiveConnection>>,
    online_connections: Mutex<HashMap<u64, OnlineConnection>>,
    switch_channels: Mutex<HashMap<u64, SwitchChannel>>,
    pending_switch: Mutex<Option<PendingSwitch>>,
    runtime_status: Mutex<RuntimeStatus>,
    ui_hwnd: Arc<AtomicIsize>,
    ui_update_pending: AtomicBool,
    update: update::UpdateManager,
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
    update: update::UpdateSnapshot,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--verify-release-installer")
    {
        let path = arguments
            .get(index + 1)
            .ok_or("missing installer path for release verification")?;
        update::verify_release_installer(std::path::Path::new(path))
            .map_err(|error| format!("release installer verification failed: {error}"))?;
        return Ok(());
    }
    diagnostics::log("startup");
    let pairing_preview = std::env::args().any(|argument| argument == "--ui-preview-pairing");
    let ui_preview = pairing_preview || std::env::args().any(|argument| argument == "--ui-preview");
    if std::env::args().any(|argument| argument == "--enable-auto-start") {
        settings::set_auto_start(true)?;
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--disable-auto-start") {
        settings::set_auto_start(false)?;
        return Ok(());
    }
    let Some(_instance) = SingleInstance::acquire()? else {
        diagnostics::log("startup existing_instance");
        ui::show_existing_window();
        return Ok(());
    };
    let identity = PcIdentity::load_or_create()?;
    let paired_phones = PairedPhoneStore::load()?;
    let first_run = paired_phones.is_empty();
    let pairing_token = first_run.then(random_token);
    let injector = InjectorDispatcher::new(injector::InjectorClient::connect().ok());
    let endpoint_host = std::env::var("FLOWTYPE_ENDPOINT_HOST")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(network_address::preferred_ipv4);
    let ui_hwnd = Arc::new(AtomicIsize::new(0));
    let update = update::UpdateManager::start(Arc::clone(&ui_hwnd))?;
    let state = Arc::new(AppState {
        identity: identity.clone(),
        pc_name: Mutex::new(identity.pc_name.clone()),
        pairing_token: Mutex::new(pairing_token),
        paired_phones,
        pairing_slot: Arc::new(Semaphore::new(1)),
        injector,
        active_connection: Mutex::new(None),
        online_connections: Mutex::new(HashMap::new()),
        switch_channels: Mutex::new(HashMap::new()),
        pending_switch: Mutex::new(None),
        runtime_status: Mutex::new(RuntimeStatus {
            summary: tr("等待手机连接", "Waiting for phone").to_owned(),
            connected_phone: None,
            target_name: None,
            last_error: None,
        }),
        ui_hwnd,
        ui_update_pending: AtomicBool::new(false),
        update,
        next_connection_id: AtomicU64::new(1),
    });

    if !ui_preview {
        let network_state = Arc::clone(&state);
        let network_error_state = Arc::clone(&state);
        std::thread::Builder::new()
            .name("flowtype-network".to_owned())
            .spawn(move || {
                let runtime =
                    tokio::runtime::Runtime::new().expect("cannot create network runtime");
                if let Err(error) = runtime.block_on(run_network(network_state, endpoint_host)) {
                    eprintln!("network stopped: {error}");
                    network_error_state.update_status(|status| {
                        status.summary = tr("连接服务不可用", "Connection unavailable").to_owned();
                        status.last_error = Some(format!(
                            "{}{error}",
                            tr(
                                "无法启动局域网连接：",
                                "Could not start the local connection: "
                            )
                        ));
                    });
                }
            })?;
    }
    let show_requested = std::env::args().any(|argument| argument == "--show");
    diagnostics::log("ui run");
    let result = ui::run(
        state,
        endpoint_host,
        first_run || show_requested || pairing_preview,
        pairing_preview,
    );
    diagnostics::log(format!("ui exit result={result:?}"));
    result?;
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

impl AppState {
    fn set_ui_hwnd(&self, hwnd: isize) {
        self.ui_hwnd.store(hwnd, Ordering::Release);
    }

    fn snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            pc_name: self
                .pc_name
                .lock()
                .map(|name| name.clone())
                .unwrap_or_else(|_| self.identity.pc_name.clone()),
            phones: self.paired_phones.snapshot(),
            status: self
                .runtime_status
                .lock()
                .map(|status| status.clone())
                .unwrap_or_else(|_| RuntimeStatus {
                    summary: tr("状态不可用", "Status unavailable").to_owned(),
                    connected_phone: None,
                    target_name: None,
                    last_error: Some(
                        tr("内部状态不可用", "Internal status unavailable").to_owned(),
                    ),
                }),
            injector_ready: self.injector.is_ready(),
            update: self.update.snapshot(),
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
        if hwnd != 0
            && !self.ui_update_pending.swap(true, Ordering::AcqRel)
            && unsafe { PostMessageW(hwnd as _, WM_APP_STATE, 0, 0) } == 0
        {
            self.ui_update_pending.store(false, Ordering::Release);
        }
    }

    fn begin_ui_update(&self) {
        self.ui_update_pending.store(false, Ordering::Release);
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
                status.summary = format!("{}{phone_name}", tr("已连接：", "Connected: "));
                status.last_error = None;
            } else {
                status.connected_phone = None;
                status.target_name = None;
                status.summary = tr("等待手机连接", "Waiting for phone").to_owned();
            }
        });
    }

    fn request_switch_to_current(self: &Arc<Self>) {
        let pc_name = self
            .pc_name
            .lock()
            .map(|name| name.clone())
            .unwrap_or_else(|_| self.identity.pc_name.clone());
        let request_id = random_token();
        let request = SwitchRequest {
            pc_id: self.identity.pc_id.clone(),
            pc_name,
            request_id: request_id.clone(),
        };
        let preferred_id = self
            .active_connection
            .lock()
            .ok()
            .and_then(|active| active.as_ref().map(|connection| connection.connection_id));
        let mut channels = match self.switch_channels.lock() {
            Ok(channels) => channels,
            Err(_) => {
                self.update_status(|status| {
                    status.summary = tr("手机未响应", "Phone did not respond").to_owned();
                });
                return;
            }
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
            drop(channels);
            self.update_status(|status| {
                status.summary = tr("手机未连接", "Phone is not connected").to_owned();
            });
            return;
        };
        let supports_switch_ack = channels
            .get(&candidate_id)
            .is_some_and(|channel| channel.supports_switch_ack);
        let send_result = channels
            .get(&candidate_id)
            .map(|channel| channel.sender.try_send(request));
        match send_result {
            Some(Ok(())) => {}
            Some(Err(tokio::sync::mpsc::error::TrySendError::Full(_))) => {
                drop(channels);
                self.update_status(|status| {
                    status.summary =
                        tr("切换请求正在处理中", "Switch request is already pending").to_owned();
                });
                return;
            }
            Some(Err(tokio::sync::mpsc::error::TrySendError::Closed(_))) | None => {
                channels.remove(&candidate_id);
                drop(channels);
                self.update_status(|status| {
                    status.summary = tr("手机未响应", "Phone did not respond").to_owned();
                });
                return;
            }
        }
        drop(channels);
        if !supports_switch_ack {
            self.update_status(|status| {
                status.summary = tr("已发送切换请求", "Switch request sent").to_owned();
            });
            return;
        }
        if let Ok(mut pending) = self.pending_switch.lock() {
            *pending = Some(PendingSwitch {
                pc_id: self.identity.pc_id.clone(),
                request_id: request_id.clone(),
            });
        }
        self.update_status(|status| {
            status.summary = tr("正在切换手机输入", "Switching phone input").to_owned();
        });
        let state = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(2_500));
            let Some(state) = state.upgrade() else { return };
            let timed_out = state
                .pending_switch
                .lock()
                .map(|mut pending| {
                    if pending
                        .as_ref()
                        .is_some_and(|value| value.request_id == request_id)
                    {
                        *pending = None;
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if timed_out {
                state.update_status(|status| {
                    status.summary = tr("手机未响应", "Phone did not respond").to_owned();
                });
            }
        });
    }

    fn acknowledge_switch(&self, ack: &SwitchAck) {
        let matched = self
            .pending_switch
            .lock()
            .map(|mut pending| {
                if pending.as_ref().is_some_and(|value| {
                    value.request_id == ack.request_id && value.pc_id == ack.pc_id
                }) {
                    *pending = None;
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if matched {
            self.update_status(|status| {
                status.summary = if ack.accepted {
                    tr("已切换到此电脑", "Switched to this computer").to_owned()
                } else {
                    tr("手机未找到这台电脑", "Computer is not paired on the phone").to_owned()
                };
            });
        }
    }

    fn register_switch_channel(
        &self,
        connection_id: u64,
        sender: Sender<SwitchRequest>,
        is_control: bool,
        supports_switch_ack: bool,
    ) {
        if let Ok(mut channels) = self.switch_channels.lock() {
            channels.insert(
                connection_id,
                SwitchChannel {
                    sender,
                    is_control,
                    supports_switch_ack,
                },
            );
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
        self.paired_phones.remove(phone_id)?;
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
                .map(|phone| format!("{}{phone}", tr("已连接：", "Connected: ")))
                .unwrap_or_else(|| tr("等待手机连接", "Waiting for phone").to_owned());
        });
    }

    fn repair_injector(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.injector.repair()?;
        self.update_status(|status| {
            status.last_error = None;
            if status.connected_phone.is_none() {
                status.summary = tr("等待手机连接", "Waiting for phone").to_owned();
            }
        });
        Ok(())
    }

    fn language_changed(&self) {
        if let Ok(mut status) = self.runtime_status.lock() {
            status.summary = status
                .connected_phone
                .as_ref()
                .map(|phone| format!("{}{phone}", tr("已连接：", "Connected: ")))
                .unwrap_or_else(|| tr("等待手机连接", "Waiting for phone").to_owned());
            status.last_error = None;
        }
        self.update.refresh_language();
        self.notify_ui();
    }

    fn perform_update_action(&self, action: update::UpdateAction) {
        self.update.perform(action);
    }

    fn open_update_repository(&self) -> std::io::Result<()> {
        self.update.open_repository()
    }

    fn open_update_history(&self) -> std::io::Result<()> {
        self.update.open_releases()
    }

    fn install_update(&self) -> Result<(), String> {
        self.update.install()
    }

    fn update_install_blocked(&self) -> bool {
        self.active_connection
            .lock()
            .map(|active| active.is_some())
            .unwrap_or(true)
    }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{Signature, SigningKey};
    use p256::pkcs8::EncodePublicKey;

    use flowtype_core::ipc::InjectorResponse;
    use flowtype_core::protocol::{Cancel, ClientSessionState, Resume};

    use super::{
        AuthMessage, ConnectionLimiter, ImageStart, MAX_CONNECTION_ATTEMPTS_PER_MINUTE,
        MAX_CONNECTIONS_PER_IP, PairedPhone, ReconcileDecision, auth_payload,
        classify_apply_recovery, classify_finish_recovery, deduplicate_paired_phones,
        upsert_paired_phone, validate_cancel_for_phone, validate_resume_for_phone,
        verify_signature,
    };

    #[test]
    fn limits_concurrent_connections_per_ip() {
        let limiter = ConnectionLimiter::new();
        let peer = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        let leases = (0..MAX_CONNECTIONS_PER_IP)
            .map(|_| limiter.try_acquire(peer).unwrap())
            .collect::<Vec<_>>();

        assert!(limiter.try_acquire(peer).is_none());
        drop(leases);
        assert!(limiter.try_acquire(peer).is_some());
    }

    #[test]
    fn limits_connection_attempt_rate_per_ip() {
        let limiter = ConnectionLimiter::new();
        let peer = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
        for _ in 0..MAX_CONNECTION_ATTEMPTS_PER_MINUTE {
            drop(limiter.try_acquire(peer).unwrap());
        }

        assert!(limiter.try_acquire(peer).is_none());
    }

    #[test]
    fn resume_and_cancel_require_the_authenticated_phone() {
        let cancel = Cancel {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            phone_id: "phone-a".to_owned(),
            session_id: "session".to_owned(),
        };
        let resume = Resume {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            phone_id: "phone-a".to_owned(),
            session_id: "session".to_owned(),
            last_ack_sequence: 0,
            sequence: 1,
            full_text: "text".to_owned(),
            session_state: ClientSessionState::Active,
        };

        assert!(validate_cancel_for_phone(&cancel, "phone-a").is_ok());
        assert!(validate_resume_for_phone(&resume, "phone-a").is_ok());
        assert!(validate_cancel_for_phone(&cancel, "phone-b").is_err());
        assert!(validate_resume_for_phone(&resume, "phone-b").is_err());
    }

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
            capabilities: Vec::new(),
            signature: STANDARD.encode(signature.to_der()),
        };
        let point = signing_key.verifying_key().to_encoded_point(false);
        let public = p256::PublicKey::from_sec1_bytes(point.as_bytes()).unwrap();
        let public_key = STANDARD.encode(public.to_public_key_der().unwrap().as_bytes());

        assert!(verify_signature("pc", &auth, "nonce", &public_key).is_ok());
        assert!(verify_signature("pc", &auth, "different", &public_key).is_err());
    }

    #[test]
    fn re_pair_updates_existing_phone_without_resetting_pair_time() {
        let mut phones = HashMap::new();
        phones.insert(
            "phone".to_owned(),
            PairedPhone {
                phone_name: "old name".to_owned(),
                public_key_spki: "old key".to_owned(),
                paired_at: 10,
                last_connected: Some(20),
            },
        );

        upsert_paired_phone(&mut phones, "phone", "new name", "new key", 30);

        assert_eq!(phones.len(), 1);
        let phone = phones.get("phone").unwrap();
        assert_eq!(phone.phone_name, "new name");
        assert_eq!(phone.public_key_spki, "new key");
        assert_eq!(phone.paired_at, 10);
        assert_eq!(phone.last_connected, Some(30));
    }

    #[test]
    fn duplicate_public_key_keeps_the_most_recent_record() {
        let mut phones = HashMap::new();
        phones.insert(
            "older".to_owned(),
            PairedPhone {
                phone_name: "same phone".to_owned(),
                public_key_spki: "same key".to_owned(),
                paired_at: 10,
                last_connected: Some(20),
            },
        );
        phones.insert(
            "newer".to_owned(),
            PairedPhone {
                phone_name: "same phone".to_owned(),
                public_key_spki: "same key".to_owned(),
                paired_at: 30,
                last_connected: Some(40),
            },
        );

        let (deduplicated, changed) = deduplicate_paired_phones(phones);

        assert!(changed);
        assert_eq!(deduplicated.len(), 1);
        assert!(deduplicated.contains_key("newer"));
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

    #[test]
    fn recognizes_an_applied_snapshot_after_reconnecting_to_the_same_injector() {
        let state = InjectorResponse::SessionActive {
            session_id: "voice".to_owned(),
            sequence: 7,
            full_text: "修正后的文本".to_owned(),
        };

        assert_eq!(
            classify_apply_recovery("voice", 7, "修正后的文本", &state),
            ReconcileDecision::Applied,
        );
    }

    #[test]
    fn retries_only_when_the_injector_is_behind_the_requested_snapshot() {
        let state = InjectorResponse::SessionActive {
            session_id: "voice".to_owned(),
            sequence: 6,
            full_text: "旧文本".to_owned(),
        };

        assert_eq!(
            classify_apply_recovery("voice", 7, "新文本", &state),
            ReconcileDecision::Retry,
        );
    }

    #[test]
    fn does_not_replay_when_the_same_sequence_has_different_text() {
        let state = InjectorResponse::SessionActive {
            session_id: "voice".to_owned(),
            sequence: 7,
            full_text: "无法确认的文本".to_owned(),
        };

        assert_eq!(
            classify_apply_recovery("voice", 7, "手机正文", &state),
            ReconcileDecision::Unknown,
        );
    }

    #[test]
    fn recognizes_a_finished_session_and_retries_an_unfinished_one() {
        let finished = InjectorResponse::SessionFinished {
            session_id: "voice".to_owned(),
            sequence: 7,
            full_text: "最终文本".to_owned(),
        };
        let active = InjectorResponse::SessionActive {
            session_id: "voice".to_owned(),
            sequence: 7,
            full_text: "最终文本".to_owned(),
        };

        assert_eq!(
            classify_finish_recovery("voice", 7, &finished),
            ReconcileDecision::Finished,
        );
        assert_eq!(
            classify_finish_recovery("voice", 7, &active),
            ReconcileDecision::Retry,
        );
    }

    #[test]
    fn recognizes_a_finished_session_after_reconnecting() {
        let finished = InjectorResponse::SessionFinished {
            session_id: "voice".to_owned(),
            sequence: 8,
            full_text: "最终正文".to_owned(),
        };

        assert_eq!(
            classify_apply_recovery("voice", 8, "最终正文", &finished),
            ReconcileDecision::Finished,
        );
        assert_eq!(
            classify_apply_recovery("voice", 8, "不同正文", &finished),
            ReconcileDecision::Unknown,
        );
    }
}
