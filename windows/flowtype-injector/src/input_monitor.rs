use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED,
    MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MOUSEHWHEEL,
    WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_XBUTTONDBLCLK,
    WM_XBUTTONDOWN,
};

static PHYSICAL_INPUT_EPOCH: AtomicU64 = AtomicU64::new(0);
static LAST_PHYSICAL_RETURN_EPOCH: AtomicU64 = AtomicU64::new(0);
static MONITOR_ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct InputMonitor {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl InputMonitor {
    pub fn start() -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || unsafe {
            // Force creation of the thread message queue before publishing its
            // id so Drop can always stop GetMessageW with WM_QUIT.
            let mut message = MSG::default();
            PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            let thread_id = GetCurrentThreadId();
            let keyboard =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), std::ptr::null_mut(), 0);
            if keyboard.is_null() || mouse.is_null() {
                let error = io::Error::last_os_error();
                if !keyboard.is_null() {
                    UnhookWindowsHookEx(keyboard);
                }
                if !mouse.is_null() {
                    UnhookWindowsHookEx(mouse);
                }
                let _ = sender.send(Err(error));
                return;
            }
            MONITOR_ACTIVE.store(true, Ordering::Release);
            if sender.send(Ok(thread_id)).is_err() {
                UnhookWindowsHookEx(keyboard);
                UnhookWindowsHookEx(mouse);
                MONITOR_ACTIVE.store(false, Ordering::Release);
                return;
            }

            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            UnhookWindowsHookEx(keyboard);
            UnhookWindowsHookEx(mouse);
            MONITOR_ACTIVE.store(false, Ordering::Release);
        });
        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "input monitor did not become ready",
            )),
        }
    }
}

impl Drop for InputMonitor {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// The monitor exists only while an ActiveSession owns it. Process-global
// counters keep the hook callbacks allocation-free.
pub fn epoch() -> u64 {
    PHYSICAL_INPUT_EPOCH.load(Ordering::Acquire)
}

pub fn last_event_was_return() -> bool {
    LAST_PHYSICAL_RETURN_EPOCH.load(Ordering::Acquire) == epoch()
        && LAST_PHYSICAL_RETURN_EPOCH.load(Ordering::Acquire) != 0
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 && lparam != 0 {
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if is_physical_keyboard_event(wparam as u32, event.flags) {
            let epoch = PHYSICAL_INPUT_EPOCH.fetch_add(1, Ordering::AcqRel) + 1;
            if event.vkCode == 0x0d {
                LAST_PHYSICAL_RETURN_EPOCH.store(epoch, Ordering::Release);
            } else {
                LAST_PHYSICAL_RETURN_EPOCH.store(0, Ordering::Release);
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 && lparam != 0 {
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if is_physical_mouse_event(wparam as u32, event.flags) {
            PHYSICAL_INPUT_EPOCH.fetch_add(1, Ordering::AcqRel);
            LAST_PHYSICAL_RETURN_EPOCH.store(0, Ordering::Release);
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn is_physical_keyboard_event(message: u32, flags: u32) -> bool {
    matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) && flags & LLKHF_INJECTED == 0
}

fn is_physical_mouse_event(message: u32, flags: u32) -> bool {
    matches!(
        message,
        WM_LBUTTONDOWN
            | WM_LBUTTONDBLCLK
            | WM_RBUTTONDOWN
            | WM_RBUTTONDBLCLK
            | WM_MBUTTONDOWN
            | WM_MBUTTONDBLCLK
            | WM_XBUTTONDOWN
            | WM_XBUTTONDBLCLK
            | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL
    ) && flags & LLMHF_INJECTED == 0
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LLKHF_INJECTED, LLMHF_INJECTED, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE,
    };

    use super::{
        InputMonitor, MONITOR_ACTIVE, Ordering, is_physical_keyboard_event, is_physical_mouse_event,
    };

    #[test]
    fn hooks_are_removed_when_the_active_session_releases_the_monitor() {
        let monitor = InputMonitor::start().unwrap();
        assert!(MONITOR_ACTIVE.load(Ordering::Acquire));

        drop(monitor);
        assert!(!MONITOR_ACTIVE.load(Ordering::Acquire));
    }

    #[test]
    fn ignores_injected_input_and_mouse_movement() {
        assert!(!is_physical_keyboard_event(WM_KEYDOWN, LLKHF_INJECTED));
        assert!(!is_physical_mouse_event(WM_LBUTTONDOWN, LLMHF_INJECTED));
        assert!(!is_physical_mouse_event(WM_MOUSEMOVE, 0));
    }

    #[test]
    fn recognizes_physical_keyboard_and_mouse_actions() {
        assert!(is_physical_keyboard_event(WM_KEYDOWN, 0));
        assert!(is_physical_mouse_event(WM_LBUTTONDOWN, 0));
    }
}
