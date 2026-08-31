use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::mem::size_of;
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use flowtype_core::ipc::{read_message, write_message};
use flowtype_core::tip::{TIP_PIPE_NAME, TipCommand, TipHello, TipResponse};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};

use crate::diagnostics;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
const TIP_RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_500);
const TIP_RETRY_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TipKey {
    pub process_id: u32,
    pub thread_id: u32,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipBeginError {
    Unavailable,
    Unsupported,
    RebindRejected,
}

pub struct TipBegin<'a> {
    pub session_id: &'a str,
    pub sequence: i64,
    pub full_text: &'a str,
    pub attach_existing: bool,
}

#[derive(Clone)]
struct Client {
    key: TipKey,
    sender: mpsc::SyncSender<BrokerRequest>,
}

struct BrokerRequest {
    command: TipCommand,
    response: mpsc::Sender<io::Result<TipResponse>>,
}

#[derive(Default)]
struct RegistryState {
    clients: HashMap<(u32, u32), Client>,
}

#[derive(Default)]
pub struct TipRegistry {
    state: Mutex<RegistryState>,
    changed: Condvar,
}

impl TipRegistry {
    pub fn start(pipe_sddl: Vec<u16>) -> Arc<Self> {
        let registry = Arc::new(Self::default());
        let listener_registry = registry.clone();
        thread::spawn(move || listener_registry.listen(pipe_sddl));
        registry
    }

    pub fn begin_for_target(
        &self,
        process_id: u32,
        preferred_thread_id: u32,
        begin: TipBegin<'_>,
        timeout: Duration,
    ) -> Result<TipKey, TipBeginError> {
        let deadline = Instant::now() + timeout;
        let mut target_rejected = false;
        loop {
            let mut candidates = self.candidates(process_id);
            candidates.sort_by_key(|client| client.key.thread_id != preferred_thread_id);
            for client in candidates {
                let response = self.send_to(
                    &client,
                    TipCommand::Begin {
                        session_id: begin.session_id.to_owned(),
                        sequence: begin.sequence,
                        full_text: begin.full_text.to_owned(),
                        attach_existing: begin.attach_existing,
                    },
                );
                diagnostics::log(format!(
                    "tip_begin candidate pid={} thread={} response={response:?}",
                    client.key.process_id, client.key.thread_id
                ));
                if matches!(
                    &response,
                    Ok(TipResponse::Begun { session_id: begun }) if begun == begin.session_id
                ) {
                    return Ok(client.key);
                }
                if matches!(
                    &response,
                    Ok(TipResponse::NoFocus | TipResponse::EditRejected)
                ) {
                    target_rejected = true;
                }
                if matches!(&response, Ok(TipResponse::RebindRejected)) {
                    return Err(TipBeginError::RebindRejected);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(if target_rejected {
                    TipBeginError::Unsupported
                } else {
                    TipBeginError::Unavailable
                });
            }
            let state = self.state.lock().map_err(|_| TipBeginError::Unavailable)?;
            let wait = deadline
                .saturating_duration_since(now)
                .min(TIP_RETRY_INTERVAL);
            let _ = self
                .changed
                .wait_timeout(state, wait)
                .map_err(|_| TipBeginError::Unavailable)?;
        }
    }

    pub fn send(&self, key: TipKey, command: TipCommand) -> io::Result<TipResponse> {
        let client = self
            .state
            .lock()
            .map_err(poisoned)?
            .clients
            .get(&(key.process_id, key.thread_id))
            .filter(|client| client.key.generation == key.generation)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "TSF client left"))?;
        self.send_to(&client, command)
    }

    fn send_to(&self, client: &Client, command: TipCommand) -> io::Result<TipResponse> {
        let (sender, receiver) = mpsc::channel();
        client
            .sender
            .send(BrokerRequest {
                command,
                response: sender,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "TSF client left"))?;
        receiver
            .recv_timeout(TIP_RESPONSE_TIMEOUT)
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TSF edit timed out"))?
    }

    fn candidates(&self, process_id: u32) -> Vec<Client> {
        self.state
            .lock()
            .map(|state| {
                state
                    .clients
                    .values()
                    .filter(|client| client.key.process_id == process_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn listen(self: Arc<Self>, pipe_sddl: Vec<u16>) {
        loop {
            let Ok(pipe) = accept_tip_pipe(&pipe_sddl) else {
                continue;
            };
            let registry = self.clone();
            thread::spawn(move || registry.serve_client(pipe));
        }
    }

    fn serve_client(&self, mut pipe: File) {
        let Ok(hello) = read_message::<TipHello>(&mut pipe) else {
            return;
        };
        if !hello_is_compatible(&hello) {
            diagnostics::log(format!(
                "tip_client rejected pid={} thread={} reason=component_mismatch ipc={} component={}",
                hello.process_id, hello.thread_id, hello.ipc_version, hello.component_version
            ));
            quarantine_incompatible_client(&mut pipe);
            return;
        }
        if !pipe_client_matches(&pipe, hello.process_id) {
            diagnostics::log(format!(
                "tip_client rejected pid={} thread={} reason=pipe_pid_mismatch",
                hello.process_id, hello.thread_id
            ));
            return;
        }
        diagnostics::log(format!(
            "tip_client connected pid={} thread={} ipc={} component={}",
            hello.process_id, hello.thread_id, hello.ipc_version, hello.component_version
        ));
        let key = TipKey {
            process_id: hello.process_id,
            thread_id: hello.thread_id,
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        };
        let (sender, receiver) = mpsc::sync_channel::<BrokerRequest>(8);
        if let Ok(mut state) = self.state.lock() {
            state
                .clients
                .insert((key.process_id, key.thread_id), Client { key, sender });
            self.changed.notify_all();
        }
        for request in receiver {
            let result = write_message(&mut pipe, &request.command)
                .and_then(|_| read_message::<TipResponse>(&mut pipe));
            let failed = result.is_err();
            let _ = request.response.send(result);
            if failed {
                break;
            }
        }
        if let Ok(mut state) = self.state.lock()
            && state
                .clients
                .get(&(key.process_id, key.thread_id))
                .is_some_and(|client| client.key.generation == key.generation)
        {
            state.clients.remove(&(key.process_id, key.thread_id));
            self.changed.notify_all();
        }
        diagnostics::log(format!(
            "tip_client disconnected pid={} thread={}",
            key.process_id, key.thread_id
        ));
    }
}

fn accept_tip_pipe(sddl: &[u16]) -> io::Result<File> {
    let name: Vec<u16> = TIP_PIPE_NAME.encode_utf16().chain(Some(0)).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            255,
            (flowtype_core::MAX_MESSAGE_BYTES + 4) as u32,
            (flowtype_core::MAX_MESSAGE_BYTES + 4) as u32,
            0,
            &attributes,
        )
    };
    unsafe { LocalFree(descriptor) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
        unsafe { CloseHandle(handle) };
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn pipe_client_matches(pipe: &File, expected_process_id: u32) -> bool {
    use std::os::windows::io::AsRawHandle;

    let mut process_id = 0_u32;
    unsafe {
        GetNamedPipeClientProcessId(pipe.as_raw_handle() as HANDLE, &mut process_id) != 0
            && process_id == expected_process_id
    }
}

fn quarantine_incompatible_client(pipe: &mut File) {
    // Old in-process TIP DLLs cannot be unloaded until their host application
    // exits. Keeping the rejected pipe open prevents those DLLs from entering
    // their reconnect loop and consuming CPU on the user's input thread.
    let mut byte = [0_u8; 1];
    let _ = pipe.read(&mut byte);
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("TSF registry state unavailable")
}

fn hello_is_compatible(hello: &TipHello) -> bool {
    // A TSF host can keep an older DLL loaded after an upgrade. The protocol
    // version is the compatibility boundary; the application version is only
    // diagnostic metadata and must not strand those long-lived host processes.
    hello.ipc_version == flowtype_core::TIP_IPC_VERSION
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use flowtype_core::tip::{TipCommand, TipHello, TipResponse, TipSessionModel};

    use super::{
        BrokerRequest, Client, TipBegin, TipBeginError, TipKey, TipRegistry, hello_is_compatible,
    };

    #[test]
    fn accepts_current_protocol_from_an_older_loaded_component() {
        let mut hello = TipHello {
            ipc_version: flowtype_core::TIP_IPC_VERSION,
            component_version: "0.2.1".to_owned(),
            process_id: 1,
            thread_id: 2,
        };
        assert!(hello_is_compatible(&hello));

        hello.ipc_version -= 1;
        assert!(!hello_is_compatible(&hello));
    }

    #[test]
    fn rejects_tip_versions_without_single_session_range_ownership() {
        assert_eq!(flowtype_core::TIP_IPC_VERSION, 8);
        for ipc_version in [3, 4, 5, 6, 7] {
            let hello = TipHello {
                ipc_version,
                component_version: "0.2.7".to_owned(),
                process_id: 1,
                thread_id: 2,
            };
            assert!(!hello_is_compatible(&hello));
        }
    }

    #[test]
    fn one_tip_connection_preserves_the_full_session_lifecycle() {
        let registry = TipRegistry::default();
        let key = TipKey {
            process_id: 70,
            thread_id: 80,
            generation: 1,
        };
        let (sender, receiver) = mpsc::sync_channel::<BrokerRequest>(8);
        registry
            .state
            .lock()
            .unwrap()
            .clients
            .insert((key.process_id, key.thread_id), Client { key, sender });
        let worker = std::thread::spawn(move || {
            let mut model = TipSessionModel::default();
            for request in receiver {
                let response = match request.command {
                    TipCommand::Begin {
                        session_id,
                        sequence,
                        full_text,
                        ..
                    } => model
                        .begin(&session_id, sequence, &full_text)
                        .map(|()| TipResponse::Begun { session_id })
                        .unwrap_or_else(|response| response),
                    TipCommand::Update {
                        session_id,
                        sequence,
                        full_text,
                    } => model
                        .update(&session_id, sequence, &full_text)
                        .map(|_| TipResponse::Applied {
                            session_id,
                            sequence,
                        })
                        .unwrap_or_else(|response| response),
                    TipCommand::Finish {
                        session_id,
                        sequence,
                    } => model
                        .finish(&session_id, sequence)
                        .map(|()| TipResponse::Finished {
                            session_id,
                            sequence,
                        })
                        .unwrap_or_else(|response| response),
                    other => panic!("unexpected command: {other:?}"),
                };
                request.response.send(Ok(response)).unwrap();
            }
        });

        let session_id = "voice";
        let active = registry
            .begin_for_target(
                70,
                80,
                TipBegin {
                    session_id,
                    sequence: 1,
                    full_text: "上山打老虎",
                    attach_existing: false,
                },
                Duration::from_millis(100),
            )
            .unwrap();
        assert_eq!(active, key);
        for (sequence, text) in [
            (1, "上山打老虎"),
            (2, "上山打老虎，老虎"),
            (3, "上山打老虎，老虎没打到"),
            (4, "上山打老虎"),
        ] {
            assert_eq!(
                registry
                    .send(
                        active,
                        TipCommand::Update {
                            session_id: session_id.to_owned(),
                            sequence,
                            full_text: text.to_owned(),
                        },
                    )
                    .unwrap(),
                TipResponse::Applied {
                    session_id: session_id.to_owned(),
                    sequence,
                }
            );
        }
        assert_eq!(
            registry
                .send(
                    active,
                    TipCommand::Finish {
                        session_id: session_id.to_owned(),
                        sequence: 4,
                    },
                )
                .unwrap(),
            TipResponse::Finished {
                session_id: session_id.to_owned(),
                sequence: 4,
            }
        );

        drop(registry);
        worker.join().unwrap();
    }

    #[test]
    fn retries_the_preferred_tip_client_until_it_accepts_begin() {
        let registry = TipRegistry::default();
        let key = TipKey {
            process_id: 10,
            thread_id: 20,
            generation: 1,
        };
        let (sender, receiver) = mpsc::sync_channel::<BrokerRequest>(8);
        registry
            .state
            .lock()
            .unwrap()
            .clients
            .insert((key.process_id, key.thread_id), Client { key, sender });
        let attempts = Arc::new(AtomicUsize::new(0));
        let worker_attempts = attempts.clone();
        let worker = std::thread::spawn(move || {
            for request in receiver {
                let attempt = worker_attempts.fetch_add(1, Ordering::Relaxed);
                let response = if attempt == 0 {
                    TipResponse::SessionMismatch
                } else {
                    TipResponse::Begun {
                        session_id: "voice".to_owned(),
                    }
                };
                request.response.send(Ok(response)).unwrap();
                if attempt > 0 {
                    break;
                }
            }
        });

        assert_eq!(
            registry
                .begin_for_target(
                    10,
                    20,
                    TipBegin {
                        session_id: "voice",
                        sequence: 1,
                        full_text: "正文",
                        attach_existing: false,
                    },
                    Duration::from_millis(500)
                )
                .unwrap(),
            key,
        );
        worker.join().unwrap();
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn reports_a_connected_tip_without_text_focus_as_unsupported() {
        let registry = TipRegistry::default();
        let key = TipKey {
            process_id: 30,
            thread_id: 40,
            generation: 1,
        };
        let (sender, receiver) = mpsc::sync_channel::<BrokerRequest>(8);
        registry
            .state
            .lock()
            .unwrap()
            .clients
            .insert((key.process_id, key.thread_id), Client { key, sender });
        let worker = std::thread::spawn(move || {
            for request in receiver {
                request.response.send(Ok(TipResponse::NoFocus)).unwrap();
            }
        });

        assert_eq!(
            registry.begin_for_target(
                30,
                40,
                TipBegin {
                    session_id: "voice",
                    sequence: 1,
                    full_text: "正文",
                    attach_existing: false,
                },
                Duration::from_millis(150)
            ),
            Err(TipBeginError::Unsupported),
        );
        drop(registry);
        worker.join().unwrap();
    }

    #[test]
    fn reports_no_compatible_tip_as_unavailable() {
        let registry = TipRegistry::default();

        assert_eq!(
            registry.begin_for_target(
                50,
                60,
                TipBegin {
                    session_id: "voice",
                    sequence: 1,
                    full_text: "正文",
                    attach_existing: false,
                },
                Duration::from_millis(20),
            ),
            Err(TipBeginError::Unavailable),
        );
    }
}
