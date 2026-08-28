use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use flowtype_core::ipc::{read_message, write_message};
use flowtype_core::tip::{TIP_PIPE_NAME, TipCommand, TipHello, TipResponse};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::IO::CancelSynchronousIo;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::service::WM_TIP_COMMAND;

pub struct PendingCommand {
    pub command: TipCommand,
    pub response: mpsc::Sender<TipResponse>,
}

pub struct Worker {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(window: HWND, process_id: u32, thread_id: u32) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let window_value = window.0 as isize;
        let thread = thread::spawn(move || {
            run(
                HWND(window_value as *mut _),
                process_id,
                thread_id,
                worker_running,
            );
        });
        Self {
            running,
            thread: Some(thread),
        }
    }

    pub fn stop(mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            while !thread.is_finished() {
                let _ = unsafe { CancelSynchronousIo(HANDLE(thread.as_raw_handle())) };
                thread::sleep(Duration::from_millis(1));
            }
            let _ = thread.join();
        }
    }
}

fn run(window: HWND, process_id: u32, thread_id: u32, running: Arc<AtomicBool>) {
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
            let pending = Box::new(PendingCommand {
                command,
                response: sender,
            });
            let raw = Box::into_raw(pending);
            if unsafe {
                PostMessageW(
                    Some(window),
                    WM_TIP_COMMAND,
                    WPARAM(0),
                    LPARAM(raw as isize),
                )
            }
            .is_err()
            {
                unsafe { drop(Box::from_raw(raw)) };
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
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};

    use super::Worker;

    #[test]
    fn stop_cancels_a_blocking_pipe_read() {
        let worker = Worker::start(HWND::default(), unsafe { GetCurrentProcessId() }, unsafe {
            GetCurrentThreadId()
        });
        std::thread::sleep(Duration::from_millis(50));

        let started = Instant::now();
        worker.stop();

        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
