use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flowtype_core::ipc::{read_message, write_message};
use flowtype_core::tip::{TIP_PIPE_NAME, TipCommand, TipHello, TipResponse};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::IO::CancelSynchronousIo;
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::service::WM_TIP_COMMAND;

const STOP_TIMEOUT: Duration = Duration::from_millis(250);

struct PendingCommand {
    pub command: TipCommand,
    pub response: mpsc::Sender<TipResponse>,
}

#[derive(Clone, Default)]
pub struct PendingCommands {
    commands: Arc<Mutex<HashMap<usize, PendingCommand>>>,
}

impl PendingCommands {
    fn insert(&self, pending: PendingCommand) -> Option<usize> {
        let mut commands = self.commands.lock().ok()?;
        for _ in 0..16 {
            let token = rand::random::<u64>() as usize;
            if token != 0 && !commands.contains_key(&token) {
                commands.insert(token, pending);
                return Some(token);
            }
        }
        None
    }

    pub fn take(&self, token: usize) -> Option<(TipCommand, mpsc::Sender<TipResponse>)> {
        self.commands
            .lock()
            .ok()?
            .remove(&token)
            .map(|pending| (pending.command, pending.response))
    }

    fn remove(&self, token: usize) {
        if let Ok(mut commands) = self.commands.lock() {
            commands.remove(&token);
        }
    }

    fn try_clear(&self) {
        if let Ok(mut commands) = self.commands.try_lock() {
            commands.clear();
        }
    }
}

pub struct Worker {
    running: Arc<AtomicBool>,
    pending: PendingCommands,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(window: HWND, process_id: u32, thread_id: u32, pending: PendingCommands) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let worker_pending = pending.clone();
        let window_value = window.0 as isize;
        let thread = thread::spawn(move || {
            run(
                HWND(window_value as *mut _),
                process_id,
                thread_id,
                worker_running,
                worker_pending,
            );
        });
        Self {
            running,
            pending,
            thread: Some(thread),
        }
    }

    pub fn stop(mut self) {
        if crate::lifetime::process_is_terminating() {
            // ExitProcess has already terminated the other threads. Their Rust
            // completion flags and held mutexes may never be released. The OS
            // will reclaim these resources; do not wait or run their cleanup.
            std::mem::forget(self);
            return;
        }
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let handle = HANDLE(thread.as_raw_handle());
            let deadline = Instant::now() + STOP_TIMEOUT;
            loop {
                match unsafe { WaitForSingleObject(handle, 0) } {
                    WAIT_OBJECT_0 => break,
                    WAIT_TIMEOUT if Instant::now() < deadline => {
                        // Reissue cancellation to cover a read starting just
                        // after running=false and the preceding cancel call.
                        let _ = unsafe { CancelSynchronousIo(handle) };
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        let wait_ms = remaining.as_millis().min(10) as u32;
                        let _ = unsafe { WaitForSingleObject(handle, wait_ms) };
                    }
                    _ => {
                        // Deactivate can run under the loader lock. A worker
                        // may need that lock to exit, so joining here would
                        // deadlock. Returning must not let COM unload its code.
                        crate::lifetime::keep_loaded_until_process_exit();
                        break;
                    }
                }
            }
            // The OS thread handle is authoritative, including threads ended
            // without running Rust destructors. Never join its Rust packet.
            drop(thread);
        }
        self.pending.try_clear();
    }
}

fn run(
    window: HWND,
    process_id: u32,
    thread_id: u32,
    running: Arc<AtomicBool>,
    pending_commands: PendingCommands,
) {
    while running.load(Ordering::Acquire) {
        let Ok(mut pipe) = open_pipe() else {
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        let hello = TipHello {
            ipc_version: flowtype_core::TIP_IPC_VERSION,
            component_version: env!("CARGO_PKG_VERSION").to_owned(),
            process_id,
            thread_id,
        };
        if write_message(&mut pipe, &hello).is_err() {
            continue;
        }
        while running.load(Ordering::Acquire) {
            let Ok(command) = read_message::<TipCommand>(&mut pipe) else {
                break;
            };
            let (sender, receiver) = mpsc::channel();
            let pending = PendingCommand {
                command,
                response: sender,
            };
            let Some(token) = pending_commands.insert(pending) else {
                break;
            };
            if unsafe { PostMessageW(Some(window), WM_TIP_COMMAND, WPARAM(token), LPARAM(0)) }
                .is_err()
            {
                pending_commands.remove(token);
                break;
            }
            let response = loop {
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(response) => break Some(response),
                    Err(mpsc::RecvTimeoutError::Timeout) if running.load(Ordering::Acquire) => {}
                    Err(_) => break None,
                }
            };
            let Some(response) = response else {
                pending_commands.remove(token);
                break;
            };
            if write_message(&mut pipe, &response).is_err() {
                break;
            }
        }
    }
}

fn open_pipe() -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(TIP_PIPE_NAME)
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::io::FromRawHandle;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::CreatePipe;
    use windows::Win32::System::SystemServices::DLL_PROCESS_DETACH;

    use super::{PendingCommand, PendingCommands, Worker};

    #[test]
    fn pending_commands_ignore_unknown_and_replayed_tokens() {
        let pending = PendingCommands::default();
        let (sender, _receiver) = std::sync::mpsc::channel();
        let token = pending
            .insert(PendingCommand {
                command: flowtype_core::tip::TipCommand::Ping,
                response: sender,
            })
            .unwrap();

        assert!(pending.take(token.wrapping_add(1)).is_none());
        assert!(pending.take(token).is_some());
        assert!(pending.take(token).is_none());
    }

    #[test]
    fn stop_cancels_a_blocking_pipe_read() {
        // A private real Windows pipe: never connect tests to the installed
        // Injector or require a phone/production pipe to be present.
        let mut read = HANDLE::default();
        let mut write = HANDLE::default();
        unsafe { CreatePipe(&mut read, &mut write, None, 0) }.unwrap();
        let mut read = unsafe { File::from_raw_handle(read.0) };
        let _write = unsafe { File::from_raw_handle(write.0) };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let error = read.read_exact(&mut [0_u8]).unwrap_err();
            done_tx.send(error.raw_os_error()).unwrap();
        });
        let worker = Worker {
            running: Arc::new(AtomicBool::new(true)),
            pending: PendingCommands::default(),
            thread: Some(thread),
        };
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let started = Instant::now();
        worker.stop();

        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Some(995)
        );
    }

    fn blocked_worker() -> (Worker, mpsc::Sender<()>, mpsc::Receiver<()>) {
        let pending = PendingCommands::default();
        let thread_pending = pending.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _lock = thread_pending.commands.lock().unwrap();
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv();
            let _ = done_tx.send(());
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        (
            Worker {
                running: Arc::new(AtomicBool::new(true)),
                pending,
                thread: Some(thread),
            },
            release_tx,
            done_rx,
        )
    }

    fn isolated_case(name: &str) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("FLOWTYPE_TIP_STOP_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(73), "child scenario: {name}");
                return;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("worker shutdown hung in isolated scenario: {name}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn stop_has_a_deadline_and_keeps_live_worker_code_loaded() {
        if std::env::var_os("FLOWTYPE_TIP_STOP_CHILD").is_none() {
            isolated_case("ipc::tests::stop_has_a_deadline_and_keeps_live_worker_code_loaded");
            return;
        }
        let (worker, release, done) = blocked_worker();
        let started = Instant::now();
        worker.stop();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            crate::DllCanUnloadNow(),
            windows::Win32::Foundation::S_FALSE
        );
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(2)).unwrap();
        std::process::exit(73);
    }

    static EXIT_WORKER: AtomicPtr<Worker> = AtomicPtr::new(std::ptr::null_mut());

    // Execute the same cleanup path as TSF during LdrShutdownProcess, after
    // Windows has killed the worker without running its Rust destructors.
    #[used]
    #[unsafe(link_section = ".CRT$XLB")]
    static EXIT_CALLBACK: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) = exit_callback;

    unsafe extern "system" fn exit_callback(_: *mut c_void, reason: u32, _: *mut c_void) {
        if reason == DLL_PROCESS_DETACH {
            let worker = EXIT_WORKER.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !worker.is_null() {
                unsafe { *Box::from_raw(worker) }.stop();
            }
        }
    }

    #[test]
    fn process_exit_does_not_wait_for_a_terminated_worker() {
        if std::env::var_os("FLOWTYPE_TIP_STOP_CHILD").is_none() {
            isolated_case("ipc::tests::process_exit_does_not_wait_for_a_terminated_worker");
            return;
        }
        let (worker, _release, _done) = blocked_worker();
        EXIT_WORKER.store(Box::into_raw(Box::new(worker)), Ordering::Release);
        std::process::exit(73);
    }
}
