use std::fs::{self, File};
use std::io::{self, Read};
use std::mem::{size_of, zeroed};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows::Win32::Networking::BackgroundIntelligentTransferService::{
    BG_JOB_PRIORITY_NORMAL, BG_JOB_PROGRESS, BG_JOB_STATE_CONNECTING, BG_JOB_STATE_ERROR,
    BG_JOB_STATE_QUEUED, BG_JOB_STATE_SUSPENDED, BG_JOB_STATE_TRANSFERRED,
    BG_JOB_STATE_TRANSFERRING, BG_JOB_STATE_TRANSIENT_ERROR, BG_JOB_TYPE_DOWNLOAD,
    BackgroundCopyManager, IBackgroundCopyJob, IBackgroundCopyManager,
};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::{GUID, HSTRING};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::Security::Cryptography::{
    CERT_SHA256_HASH_PROP_ID, CertGetCertificateContextProperty,
};
use windows_sys::Win32::Security::WinTrust::*;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};

const MANIFEST_URL: &str =
    "https://github.com/Henry10088/FlowType/releases/latest/download/flowtype-update.json";
const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/Henry10088/FlowType/releases/download/";
const RELEASE_TAG_PREFIX: &str = "https://github.com/Henry10088/FlowType/releases/tag/";
const REPOSITORY_URL: &str = "https://github.com/Henry10088/FlowType";
const RELEASES_URL: &str = "https://github.com/Henry10088/FlowType/releases";
const UPDATE_KEY_ID: &str = "flowtype-update-2026";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_SIGNATURE_BYTES: usize = 1024;
const MAX_INSTALLER_BYTES: u64 = 200 * 1024 * 1024;
const CHECK_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
const AUTO_CHECK_DELAY: Duration = Duration::from_secs(30);
const UPDATE_STATE_FILE: &str = "update-state-v1.json";
pub const WM_APP_UPDATE: u32 = 0x8005;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    None,
    Check,
    Download,
    Cancel,
    Install,
}

#[derive(Clone, Debug)]
pub struct UpdateSnapshot {
    pub message: String,
    pub action: UpdateAction,
    pub action_label: String,
    pub progress: Option<(u64, u64)>,
    pub version: Option<String>,
}

impl UpdateSnapshot {
    fn idle() -> Self {
        Self {
            message: "可检查更新".to_owned(),
            action: UpdateAction::Check,
            action_label: "检查更新".to_owned(),
            progress: None,
            version: None,
        }
    }

    pub fn tray_label(&self) -> Option<String> {
        match self.action {
            UpdateAction::Download => self.version.as_ref().map(|v| format!("更新到 {v}...")),
            UpdateAction::Cancel => Some(self.message.clone()),
            UpdateAction::Install => Some("更新已下载...".to_owned()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UpdateManifest {
    schema: u32,
    key_id: String,
    version: String,
    published_at: String,
    release_url: String,
    notes_zh_cn: String,
    windows: PlatformAsset,
    android: AndroidAsset,
    #[serde(default)]
    verified_raw: Vec<u8>,
    #[serde(default)]
    verified_signature: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PlatformAsset {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AndroidAsset {
    version_code: u64,
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct PersistedUpdate {
    last_successful_check: u64,
    highest_verified_version: Option<String>,
    job_id: Option<String>,
    manifest: Option<UpdateManifest>,
    installer_path: Option<PathBuf>,
    verified: bool,
}

struct SharedUpdate {
    snapshot: UpdateSnapshot,
    manifest: Option<UpdateManifest>,
    installer_path: Option<PathBuf>,
}

enum Command {
    Check,
    Download,
    Cancel,
}

pub struct UpdateManager {
    shared: Arc<Mutex<SharedUpdate>>,
    commands: Sender<Command>,
    ui_hwnd: Arc<AtomicIsize>,
}

impl UpdateManager {
    pub fn start(ui_hwnd: Arc<AtomicIsize>) -> io::Result<Self> {
        let shared = Arc::new(Mutex::new(SharedUpdate {
            snapshot: UpdateSnapshot::idle(),
            manifest: None,
            installer_path: None,
        }));
        let (sender, receiver) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker_hwnd = Arc::clone(&ui_hwnd);
        std::thread::Builder::new()
            .name("flowtype-update".to_owned())
            .spawn(move || update_worker(worker_shared, receiver, worker_hwnd))?;
        Ok(Self {
            shared,
            commands: sender,
            ui_hwnd,
        })
    }

    pub fn snapshot(&self) -> UpdateSnapshot {
        self.shared
            .lock()
            .map(|state| state.snapshot.clone())
            .unwrap_or_else(|_| UpdateSnapshot {
                message: "更新状态不可用".to_owned(),
                action: UpdateAction::Check,
                action_label: "重试检查更新".to_owned(),
                progress: None,
                version: None,
            })
    }

    pub fn perform(&self, action: UpdateAction) {
        let command = match action {
            UpdateAction::Check => Some(Command::Check),
            UpdateAction::Download => Some(Command::Download),
            UpdateAction::Cancel => Some(Command::Cancel),
            _ => None,
        };
        if let Some(command) = command {
            let _ = self.commands.send(command);
        }
    }

    pub fn open_repository(&self) -> io::Result<()> {
        shell_open("open", REPOSITORY_URL)
    }

    pub fn open_releases(&self) -> io::Result<()> {
        shell_open("open", RELEASES_URL)
    }

    pub fn install(&self) -> Result<(), String> {
        let (manifest, path) = self
            .shared
            .lock()
            .map_err(|_| "更新状态不可用".to_owned())
            .and_then(|state| {
                Ok((
                    state
                        .manifest
                        .clone()
                        .ok_or_else(|| "更新清单不可用".to_owned())?,
                    state
                        .installer_path
                        .clone()
                        .ok_or_else(|| "安装包不可用".to_owned())?,
                ))
            })?;
        set_snapshot(
            &self.shared,
            &self.ui_hwnd,
            UpdateSnapshot {
                message: "正在重新校验更新…".to_owned(),
                action: UpdateAction::None,
                action_label: String::new(),
                progress: None,
                version: Some(manifest.version.clone()),
            },
            Some(manifest.clone()),
            Some(path.clone()),
        );
        verify_installer(&path, &manifest.windows).map_err(|error| {
            set_failure(
                &self.shared,
                &self.ui_hwnd,
                &manifest,
                &format!("更新校验失败：{error}"),
            );
            error
        })?;
        shell_open("runas", path.to_string_lossy().as_ref()).map_err(|error| {
            set_failure(
                &self.shared,
                &self.ui_hwnd,
                &manifest,
                &format!("无法启动安装程序：{error}"),
            );
            error.to_string()
        })
    }
}

fn update_worker(
    shared: Arc<Mutex<SharedUpdate>>,
    receiver: Receiver<Command>,
    ui_hwnd: Arc<AtomicIsize>,
) {
    let _com = ComGuard::initialize().ok();
    let mut persisted = load_persisted().unwrap_or_default();
    persisted.manifest = persisted
        .manifest
        .as_ref()
        .and_then(|cached| verify_manifest(&cached.verified_raw, &cached.verified_signature).ok());
    if persisted.manifest.is_none() {
        persisted.job_id = None;
        persisted.installer_path = None;
        persisted.verified = false;
        let _ = save_persisted(&persisted);
    }
    let mut active = restore_download(&shared, &ui_hwnd, &persisted)
        .ok()
        .flatten();
    let mut available = persisted.manifest.clone();
    let auto_deadline = Instant::now() + AUTO_CHECK_DELAY;
    let mut auto_pending = should_auto_check(&persisted);

    loop {
        let timeout = if active.is_some() {
            Duration::from_millis(500)
        } else if auto_pending {
            auto_deadline.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(60)
        };
        match receiver.recv_timeout(timeout) {
            Ok(Command::Check) => {
                auto_pending = false;
                check_for_update(&shared, &ui_hwnd, &mut persisted, &mut available);
            }
            Ok(Command::Download) => {
                if active.is_none()
                    && let Some(manifest) = available.clone()
                {
                    match start_download(&manifest, &mut persisted) {
                        Ok(download) => {
                            let snapshot =
                                downloading_snapshot(&manifest, 0, manifest.windows.size);
                            set_snapshot(
                                &shared,
                                &ui_hwnd,
                                snapshot,
                                Some(manifest),
                                Some(download.path.clone()),
                            );
                            active = Some(download);
                        }
                        Err(error) => set_download_failure(
                            &shared,
                            &ui_hwnd,
                            &manifest,
                            &format!("无法开始下载：{error}"),
                        ),
                    }
                }
            }
            Ok(Command::Cancel) => {
                if let Some(download) = active.take() {
                    let _ = unsafe { download.job.Cancel() };
                    let _ = fs::remove_file(&download.path);
                }
                persisted.job_id = None;
                persisted.installer_path = None;
                persisted.verified = false;
                let _ = save_persisted(&persisted);
                if let Some(manifest) = available.clone() {
                    set_available(&shared, &ui_hwnd, manifest);
                } else {
                    set_snapshot(&shared, &ui_hwnd, UpdateSnapshot::idle(), None, None);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if auto_pending && Instant::now() >= auto_deadline {
                    auto_pending = false;
                    check_for_update(&shared, &ui_hwnd, &mut persisted, &mut available);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if let Some(download) = active.as_ref() {
            match poll_download(download, &shared, &ui_hwnd, &mut persisted) {
                PollResult::Continue => {}
                PollResult::Ready => active = None,
                PollResult::Failed(message) => {
                    let manifest = download.manifest.clone();
                    let _ = unsafe { download.job.Cancel() };
                    active = None;
                    persisted.job_id = None;
                    persisted.verified = false;
                    let _ = save_persisted(&persisted);
                    set_download_failure(&shared, &ui_hwnd, &manifest, &message);
                }
            }
        }
    }
}

fn check_for_update(
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    persisted: &mut PersistedUpdate,
    available: &mut Option<UpdateManifest>,
) {
    set_snapshot(
        shared,
        hwnd,
        UpdateSnapshot {
            message: "正在检查更新…".to_owned(),
            action: UpdateAction::None,
            action_label: String::new(),
            progress: None,
            version: None,
        },
        None,
        None,
    );
    match fetch_verified_manifest() {
        Ok(manifest) => {
            persisted.last_successful_check = unix_time();
            if let Some(highest) = persisted.highest_verified_version.as_deref()
                && compare_versions(&manifest.version, highest).is_some_and(|order| order.is_lt())
            {
                set_failure(shared, hwnd, &manifest, "服务器返回了旧版更新清单");
                return;
            }
            if persisted
                .highest_verified_version
                .as_deref()
                .and_then(|highest| compare_versions(&manifest.version, highest))
                .is_none_or(|order| order.is_gt())
            {
                persisted.highest_verified_version = Some(manifest.version.clone());
            }
            let _ = save_persisted(persisted);
            match compare_versions(&manifest.version, env!("CARGO_PKG_VERSION")) {
                Some(order) if order.is_gt() => {
                    *available = Some(manifest.clone());
                    let ready_path = persisted
                        .installer_path
                        .as_ref()
                        .filter(|path| persisted.verified && path.is_file())
                        .filter(|path| verify_installer(path, &manifest.windows).is_ok())
                        .cloned();
                    if let Some(path) = ready_path {
                        persisted.manifest = Some(manifest.clone());
                        let _ = save_persisted(persisted);
                        set_ready(shared, hwnd, manifest, path);
                    } else {
                        set_available(shared, hwnd, manifest);
                    }
                }
                Some(_) => {
                    *available = None;
                    set_snapshot(
                        shared,
                        hwnd,
                        UpdateSnapshot {
                            message: "已是最新版本".to_owned(),
                            action: UpdateAction::Check,
                            action_label: "再次检查".to_owned(),
                            progress: None,
                            version: None,
                        },
                        None,
                        None,
                    );
                }
                None => set_failure(shared, hwnd, &manifest, "更新版本格式无效"),
            }
        }
        Err(error) => {
            let placeholder = UpdateManifest::placeholder();
            set_failure(
                shared,
                hwnd,
                &placeholder,
                &format!("检查更新失败：{}", friendly_update_error(&error)),
            );
        }
    }
}

impl UpdateManifest {
    fn placeholder() -> Self {
        Self {
            schema: 1,
            key_id: UPDATE_KEY_ID.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            published_at: String::new(),
            release_url: "https://github.com/Henry10088/FlowType/releases".to_owned(),
            notes_zh_cn: String::new(),
            windows: PlatformAsset {
                url: String::new(),
                sha256: String::new(),
                size: 0,
            },
            android: AndroidAsset {
                version_code: 0,
                url: String::new(),
                sha256: String::new(),
                size: 0,
            },
            verified_raw: Vec::new(),
            verified_signature: Vec::new(),
        }
    }
}

fn fetch_verified_manifest() -> Result<UpdateManifest, String> {
    let manifest_url =
        std::env::var("FLOWTYPE_UPDATE_MANIFEST_URL").unwrap_or_else(|_| MANIFEST_URL.to_owned());
    let bytes = http_get(&manifest_url, MAX_MANIFEST_BYTES).map_err(|e| e.to_string())?;
    let untrusted: UpdateManifest =
        serde_json::from_slice(&bytes).map_err(|_| "更新清单格式无效".to_owned())?;
    let signature_url = if manifest_url == MANIFEST_URL {
        format!(
            "{RELEASE_DOWNLOAD_PREFIX}v{}/flowtype-update.json.sig",
            untrusted.version
        )
    } else {
        let base = manifest_url
            .rsplit_once('/')
            .map(|(base, _)| base)
            .ok_or_else(|| "更新清单地址无效".to_owned())?;
        format!("{base}/flowtype-update.json.sig")
    };
    let signature = http_get(&signature_url, MAX_SIGNATURE_BYTES).map_err(|e| e.to_string())?;
    verify_manifest(&bytes, &signature)
}

fn verify_manifest(bytes: &[u8], signature_text: &[u8]) -> Result<UpdateManifest, String> {
    let public_der = STANDARD
        .decode(include_str!("../../../release/update-public-key-spki.b64").trim())
        .map_err(|_| "内置更新公钥无效".to_owned())?;
    verify_manifest_with_key(bytes, signature_text, &public_der)
}

fn verify_manifest_with_key(
    bytes: &[u8],
    signature_text: &[u8],
    public_der: &[u8],
) -> Result<UpdateManifest, String> {
    if bytes.len() > MAX_MANIFEST_BYTES || signature_text.len() > MAX_SIGNATURE_BYTES {
        return Err("更新清单过大".to_owned());
    }
    let key =
        VerifyingKey::from_public_key_der(public_der).map_err(|_| "更新公钥格式无效".to_owned())?;
    let signature_bytes = STANDARD
        .decode(String::from_utf8_lossy(signature_text).trim())
        .map_err(|_| "更新签名格式无效".to_owned())?;
    let signature =
        Signature::from_der(&signature_bytes).map_err(|_| "更新签名格式无效".to_owned())?;
    key.verify(bytes, &signature)
        .map_err(|_| "更新清单签名不匹配".to_owned())?;
    let mut manifest: UpdateManifest =
        serde_json::from_slice(bytes).map_err(|_| "更新清单格式无效".to_owned())?;
    validate_manifest(&manifest)?;
    manifest.verified_raw = bytes.to_vec();
    manifest.verified_signature = signature_text.to_vec();
    Ok(manifest)
}

fn validate_manifest(manifest: &UpdateManifest) -> Result<(), String> {
    if manifest.schema != 1 || manifest.key_id != UPDATE_KEY_ID {
        return Err("不支持的更新清单版本或密钥".to_owned());
    }
    parse_version(&manifest.version).ok_or_else(|| "更新版本格式无效".to_owned())?;
    if manifest.published_at.len() > 40
        || manifest.notes_zh_cn.len() > 8192
        || manifest.release_url.len() > 2048
    {
        return Err("更新清单字段过长".to_owned());
    }
    let tag = format!("v{}", manifest.version);
    if manifest.release_url != format!("{RELEASE_TAG_PREFIX}{tag}") {
        return Err("更新发布地址无效".to_owned());
    }
    let expected_prefix = format!("{RELEASE_DOWNLOAD_PREFIX}{tag}/");
    for asset in [
        &manifest.windows,
        &PlatformAsset {
            url: manifest.android.url.clone(),
            sha256: manifest.android.sha256.clone(),
            size: manifest.android.size,
        },
    ] {
        if !asset.url.starts_with(&expected_prefix)
            || asset.url.len() > 2048
            || asset.size == 0
            || asset.size > MAX_INSTALLER_BYTES
            || !valid_sha256(&asset.sha256)
        {
            return Err("更新资产信息无效".to_owned());
        }
    }
    if manifest.android.version_code == 0 {
        return Err("Android versionCode 无效".to_owned());
    }
    Ok(())
}

struct ActiveDownload {
    job: IBackgroundCopyJob,
    path: PathBuf,
    manifest: UpdateManifest,
}

fn start_download(
    manifest: &UpdateManifest,
    persisted: &mut PersistedUpdate,
) -> Result<ActiveDownload, String> {
    let manager = bits_manager()?;
    let update_dir = crate::identity::data_dir()
        .map_err(|e| e.to_string())?
        .join("updates")
        .join(&manifest.version);
    fs::create_dir_all(&update_dir).map_err(|e| e.to_string())?;
    let file_name = manifest
        .windows
        .url
        .rsplit_once('/')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty() && !name.contains(['\\', '/']))
        .ok_or_else(|| "安装包文件名无效".to_owned())?;
    let path = update_dir.join(file_name);
    let _ = fs::remove_file(&path);
    let mut id = GUID::zeroed();
    let mut job = None;
    unsafe {
        manager
            .CreateJob(
                &HSTRING::from(format!("FlowType {} update", manifest.version)),
                BG_JOB_TYPE_DOWNLOAD,
                &mut id,
                &mut job,
            )
            .map_err(|e| e.to_string())?;
    }
    let job = job.ok_or_else(|| "BITS 未返回下载任务".to_owned())?;
    unsafe {
        job.SetPriority(BG_JOB_PRIORITY_NORMAL)
            .map_err(|e| e.to_string())?;
        job.AddFile(
            &HSTRING::from(&manifest.windows.url),
            &HSTRING::from(path.to_string_lossy().as_ref()),
        )
        .map_err(|e| e.to_string())?;
    }
    persisted.job_id = Some(format!("{id:?}"));
    persisted.manifest = Some(manifest.clone());
    persisted.installer_path = Some(path.clone());
    persisted.verified = false;
    save_persisted(persisted).map_err(|e| e.to_string())?;
    unsafe { job.Resume().map_err(|e| e.to_string())? };
    Ok(ActiveDownload {
        job,
        path,
        manifest: manifest.clone(),
    })
}

fn restore_download(
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    persisted: &PersistedUpdate,
) -> Result<Option<ActiveDownload>, String> {
    let (Some(manifest), Some(path)) =
        (persisted.manifest.clone(), persisted.installer_path.clone())
    else {
        return Ok(None);
    };
    if persisted.verified && path.is_file() && verify_installer(&path, &manifest.windows).is_ok() {
        set_ready(shared, hwnd, manifest, path);
        return Ok(None);
    }
    let Some(id) = persisted.job_id.as_deref() else {
        return Ok(None);
    };
    let guid = GUID::try_from(id).map_err(|e| e.to_string())?;
    let manager = bits_manager()?;
    let job = unsafe { manager.GetJob(&guid).map_err(|e| e.to_string())? };
    set_snapshot(
        shared,
        hwnd,
        downloading_snapshot(&manifest, 0, manifest.windows.size),
        Some(manifest.clone()),
        Some(path.clone()),
    );
    Ok(Some(ActiveDownload {
        job,
        path,
        manifest,
    }))
}

enum PollResult {
    Continue,
    Ready,
    Failed(String),
}

fn poll_download(
    download: &ActiveDownload,
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    persisted: &mut PersistedUpdate,
) -> PollResult {
    let state = match unsafe { download.job.GetState() } {
        Ok(state) => state,
        Err(error) => return PollResult::Failed(format!("无法读取下载状态：{error}")),
    };
    if state == BG_JOB_STATE_TRANSFERRED {
        if let Err(error) = unsafe { download.job.Complete() } {
            return PollResult::Failed(format!("无法完成下载：{error}"));
        }
        set_snapshot(
            shared,
            hwnd,
            UpdateSnapshot {
                message: "正在校验更新…".to_owned(),
                action: UpdateAction::None,
                action_label: String::new(),
                progress: None,
                version: Some(download.manifest.version.clone()),
            },
            Some(download.manifest.clone()),
            Some(download.path.clone()),
        );
        if let Err(error) = verify_installer(&download.path, &download.manifest.windows) {
            let _ = fs::remove_file(&download.path);
            return PollResult::Failed(format!("更新校验失败：{error}"));
        }
        persisted.job_id = None;
        persisted.verified = true;
        let _ = save_persisted(persisted);
        set_ready(
            shared,
            hwnd,
            download.manifest.clone(),
            download.path.clone(),
        );
        return PollResult::Ready;
    }
    if state == BG_JOB_STATE_ERROR {
        return PollResult::Failed("下载失败，请重试".to_owned());
    }
    let mut progress: BG_JOB_PROGRESS = unsafe { zeroed() };
    if let Err(error) = unsafe { download.job.GetProgress(&mut progress) } {
        return PollResult::Failed(format!("无法读取下载进度：{error}"));
    }
    let total = if progress.BytesTotal == u64::MAX {
        download.manifest.windows.size
    } else {
        progress.BytesTotal
    };
    let message = if state == BG_JOB_STATE_TRANSIENT_ERROR || state == BG_JOB_STATE_SUSPENDED {
        "等待网络，恢复后继续下载".to_owned()
    } else if matches!(
        state,
        BG_JOB_STATE_QUEUED | BG_JOB_STATE_CONNECTING | BG_JOB_STATE_TRANSFERRING
    ) {
        format_progress(progress.BytesTransferred, total)
    } else {
        "正在准备下载…".to_owned()
    };
    set_snapshot(
        shared,
        hwnd,
        UpdateSnapshot {
            message,
            action: UpdateAction::Cancel,
            action_label: "取消".to_owned(),
            progress: Some((progress.BytesTransferred, total)),
            version: Some(download.manifest.version.clone()),
        },
        Some(download.manifest.clone()),
        Some(download.path.clone()),
    );
    PollResult::Continue
}

fn bits_manager() -> Result<IBackgroundCopyManager, String> {
    unsafe {
        CoCreateInstance(&BackgroundCopyManager, None, CLSCTX_LOCAL_SERVER)
            .map_err(|error| format!("BITS 服务不可用：{error}"))
    }
}

struct ComGuard;

impl ComGuard {
    fn initialize() -> windows::core::Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn set_available(
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    manifest: UpdateManifest,
) {
    set_snapshot(
        shared,
        hwnd,
        UpdateSnapshot {
            message: format!("发现新版本 {}", manifest.version),
            action: UpdateAction::Download,
            action_label: "下载更新".to_owned(),
            progress: None,
            version: Some(manifest.version.clone()),
        },
        Some(manifest),
        None,
    );
}

fn set_ready(
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    manifest: UpdateManifest,
    path: PathBuf,
) {
    set_snapshot(
        shared,
        hwnd,
        UpdateSnapshot {
            message: format!("更新 {} 已下载", manifest.version),
            action: UpdateAction::Install,
            action_label: "安装更新".to_owned(),
            progress: Some((manifest.windows.size, manifest.windows.size)),
            version: Some(manifest.version.clone()),
        },
        Some(manifest),
        Some(path),
    );
}

fn set_failure(
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    _manifest: &UpdateManifest,
    message: &str,
) {
    set_snapshot(
        shared,
        hwnd,
        UpdateSnapshot {
            message: message.to_owned(),
            action: UpdateAction::Check,
            action_label: "重试检查更新".to_owned(),
            progress: None,
            version: None,
        },
        None,
        None,
    );
}

fn set_download_failure(
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    manifest: &UpdateManifest,
    message: &str,
) {
    set_snapshot(
        shared,
        hwnd,
        UpdateSnapshot {
            message: message.to_owned(),
            action: UpdateAction::Download,
            action_label: "重试下载".to_owned(),
            progress: None,
            version: Some(manifest.version.clone()),
        },
        Some(manifest.clone()),
        None,
    );
}

fn friendly_update_error(error: &str) -> String {
    if error.contains("12002") {
        "连接 GitHub 超时，请检查网络后重试".to_owned()
    } else if error.contains("12007") {
        "无法解析 GitHub 地址，请检查网络后重试".to_owned()
    } else if error.contains("12029") || error.contains("12030") {
        "无法连接 GitHub，请检查网络后重试".to_owned()
    } else {
        error.to_owned()
    }
}

fn set_snapshot(
    shared: &Arc<Mutex<SharedUpdate>>,
    hwnd: &Arc<AtomicIsize>,
    snapshot: UpdateSnapshot,
    manifest: Option<UpdateManifest>,
    installer_path: Option<PathBuf>,
) {
    if let Ok(mut state) = shared.lock() {
        state.snapshot = snapshot;
        if manifest.is_some() {
            state.manifest = manifest;
        }
        if installer_path.is_some() {
            state.installer_path = installer_path;
        }
    }
    let window = hwnd.load(Ordering::Acquire);
    if window != 0 {
        unsafe { PostMessageW(window as HWND, WM_APP_UPDATE, 0, 0) };
    }
}

fn downloading_snapshot(manifest: &UpdateManifest, transferred: u64, total: u64) -> UpdateSnapshot {
    UpdateSnapshot {
        message: format_progress(transferred, total),
        action: UpdateAction::Cancel,
        action_label: "取消".to_owned(),
        progress: Some((transferred, total)),
        version: Some(manifest.version.clone()),
    }
}

fn format_progress(transferred: u64, total: u64) -> String {
    if total > 0 {
        let percent = transferred.saturating_mul(100) / total;
        format!(
            "正在下载 {percent}% · {:.1}/{:.1} MB",
            transferred as f64 / 1_048_576.0,
            total as f64 / 1_048_576.0
        )
    } else {
        format!("正在下载 {:.1} MB", transferred as f64 / 1_048_576.0)
    }
}

fn verify_installer(path: &Path, asset: &PlatformAsset) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() != asset.size {
        return Err("安装包大小不匹配".to_owned());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    if format!("{:x}", digest.finalize()) != asset.sha256 {
        return Err("安装包 SHA-256 不匹配".to_owned());
    }
    verify_authenticode(path)
}

fn verify_authenticode(path: &Path) -> Result<(), String> {
    let expected = option_env!("FLOWTYPE_WINDOWS_CERT_SHA256")
        .map(|value| value.replace(':', "").to_ascii_lowercase())
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "此构建未固定正式发布证书，不能安装在线更新".to_owned())?;
    let path_wide = wide(path.to_string_lossy().as_ref());
    let mut file_info: WINTRUST_FILE_INFO = unsafe { zeroed() };
    file_info.cbStruct = size_of::<WINTRUST_FILE_INFO>() as u32;
    file_info.pcwszFilePath = path_wide.as_ptr();
    let mut data: WINTRUST_DATA = unsafe { zeroed() };
    data.cbStruct = size_of::<WINTRUST_DATA>() as u32;
    data.dwUIChoice = WTD_UI_NONE;
    data.fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN;
    data.dwUnionChoice = WTD_CHOICE_FILE;
    data.Anonymous = WINTRUST_DATA_0 {
        pFile: &mut file_info,
    };
    data.dwStateAction = WTD_STATEACTION_VERIFY;
    data.dwUIContext = WTD_UICONTEXT_INSTALL;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe {
        WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    let signer_hash = if status == 0 {
        signer_certificate_sha256(data.hWVTStateData)
    } else {
        Err(String::new())
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        );
    }
    if status != 0 {
        return Err(format!("Authenticode 验证失败（0x{:08x}）", status as u32));
    }
    let actual = signer_hash?;
    if actual != expected {
        return Err("安装包签名证书不是 FlowType 正式发布证书".to_owned());
    }
    Ok(())
}

fn signer_certificate_sha256(
    state: windows_sys::Win32::Foundation::HANDLE,
) -> Result<String, String> {
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return Err("无法读取 Authenticode 验证结果".to_owned());
    }
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() {
        return Err("安装包没有 Authenticode 签名者".to_owned());
    }
    let certificate = unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if certificate.is_null() {
        return Err("无法读取 Authenticode 签名证书".to_owned());
    }
    let mut hash = [0u8; 32];
    let mut length = hash.len() as u32;
    let success = unsafe {
        CertGetCertificateContextProperty(
            (*certificate).pCert,
            CERT_SHA256_HASH_PROP_ID,
            hash.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if success == 0 || length != hash.len() as u32 {
        return Err("无法计算 Authenticode 证书指纹".to_owned());
    }
    Ok(hash.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn http_get(url: &str, limit: usize) -> io::Result<Vec<u8>> {
    if !url.starts_with("https://") || url.len() > 2048 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid HTTPS URL",
        ));
    }
    let url_wide = wide(url);
    let mut components: URL_COMPONENTS = unsafe { zeroed() };
    components.dwStructSize = size_of::<URL_COMPONENTS>() as u32;
    components.dwHostNameLength = u32::MAX;
    components.dwUrlPathLength = u32::MAX;
    components.dwExtraInfoLength = u32::MAX;
    if unsafe { WinHttpCrackUrl(url_wide.as_ptr(), 0, 0, &mut components) } == 0
        || components.nScheme != 2
    {
        return Err(io::Error::last_os_error());
    }
    let host = unsafe {
        std::slice::from_raw_parts(
            components.lpszHostName,
            components.dwHostNameLength as usize,
        )
    };
    let mut path = unsafe {
        std::slice::from_raw_parts(components.lpszUrlPath, components.dwUrlPathLength as usize)
    }
    .to_vec();
    if !components.lpszExtraInfo.is_null() && components.dwExtraInfoLength > 0 {
        path.extend_from_slice(unsafe {
            std::slice::from_raw_parts(
                components.lpszExtraInfo,
                components.dwExtraInfoLength as usize,
            )
        });
    }
    path.push(0);
    let mut host = host.to_vec();
    host.push(0);

    let agent = wide(&format!("FlowType/{}", env!("CARGO_PKG_VERSION")));
    let session = HttpHandle(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        )
    });
    session.ensure()?;
    if unsafe { WinHttpSetTimeouts(session.0, 5000, 8000, 8000, 12000) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let connection =
        HttpHandle(unsafe { WinHttpConnect(session.0, host.as_ptr(), components.nPort, 0) });
    connection.ensure()?;
    let verb = wide("GET");
    let request = HttpHandle(unsafe {
        WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    request.ensure()?;
    let headers = wide("Cache-Control: no-cache\r\nPragma: no-cache\r\n");
    if unsafe { WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX, null(), 0, 0, 0) } == 0
        || unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut status = 0u32;
    let mut status_size = size_of::<u32>() as u32;
    if unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status as *mut u32).cast(),
            &mut status_size,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if status != 200 {
        return Err(io::Error::other(format!("HTTP {status}")));
    }
    let mut result = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let mut read = 0u32;
        if unsafe {
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if read == 0 {
            break;
        }
        if result.len() + read as usize > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response is too large",
            ));
        }
        result.extend_from_slice(&buffer[..read as usize]);
    }
    Ok(result)
}

struct HttpHandle(*mut core::ffi::c_void);

impl HttpHandle {
    fn ensure(&self) -> io::Result<()> {
        if self.0.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn shell_open(verb: &str, target: &str) -> io::Result<()> {
    let verb = wide(verb);
    let target = wide(target);
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

fn should_auto_check(persisted: &PersistedUpdate) -> bool {
    unix_time().saturating_sub(persisted.last_successful_check) >= CHECK_INTERVAL_SECONDS
}

fn persisted_path() -> io::Result<PathBuf> {
    Ok(crate::identity::data_dir()?.join(UPDATE_STATE_FILE))
}

fn load_persisted() -> io::Result<PersistedUpdate> {
    let path = persisted_path()?;
    if !path.exists() {
        return Ok(PersistedUpdate::default());
    }
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn save_persisted(value: &PersistedUpdate) -> io::Result<()> {
    let path = persisted_path()?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temporary, path)
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() {
        None
    } else {
        Some(version)
    }
}

fn compare_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    Some(parse_version(left)?.cmp(&parse_version(right)?))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::SigningKey;
    use p256::ecdsa::signature::Signer;
    use p256::pkcs8::EncodePublicKey;

    fn manifest() -> UpdateManifest {
        let version = "9.8.7";
        let base = format!("{RELEASE_DOWNLOAD_PREFIX}v{version}/");
        UpdateManifest {
            schema: 1,
            key_id: UPDATE_KEY_ID.to_owned(),
            version: version.to_owned(),
            published_at: "2026-08-26T10:00:00Z".to_owned(),
            release_url: format!("{RELEASE_TAG_PREFIX}v{version}"),
            notes_zh_cn: "测试".to_owned(),
            windows: PlatformAsset {
                url: format!("{base}FlowType-{version}-x64-setup.exe"),
                sha256: "a".repeat(64),
                size: 123,
            },
            android: AndroidAsset {
                version_code: 999,
                url: format!("{base}FlowType-{version}-android-release.apk"),
                sha256: "b".repeat(64),
                size: 456,
            },
            verified_raw: Vec::new(),
            verified_signature: Vec::new(),
        }
    }

    #[test]
    fn verifies_signed_manifest_and_rejects_tampering() {
        use p256::elliptic_curve::rand_core::OsRng;

        let key = SigningKey::random(&mut OsRng);
        let bytes = serde_json::to_vec(&manifest()).unwrap();
        let signature: Signature = key.sign(&bytes);
        let signature = STANDARD.encode(signature.to_der().as_bytes());
        let public = p256::PublicKey::from(key.verifying_key())
            .to_public_key_der()
            .unwrap();
        assert!(verify_manifest_with_key(&bytes, signature.as_bytes(), public.as_bytes()).is_ok());
        let mut changed = bytes;
        changed.push(b' ');
        assert!(
            verify_manifest_with_key(&changed, signature.as_bytes(), public.as_bytes()).is_err()
        );
    }

    #[test]
    fn compares_strict_three_part_versions() {
        assert_eq!(
            compare_versions("0.2.0", "0.1.18"),
            Some(std::cmp::Ordering::Greater)
        );
        assert!(compare_versions("0.2", "0.1.18").is_none());
        assert!(compare_versions("0.2.0-beta", "0.1.18").is_none());
    }
}
