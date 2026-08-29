use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED,
    MSG, MSLLHOOKSTRUCT, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN,
    WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_SYSKEYDOWN,
    WM_XBUTTONDBLCLK, WM_XBUTTONDOWN,
};

static PHYSICAL_INPUT_EPOCH: AtomicU64 = AtomicU64::new(0);
static LAST_PHYSICAL_RETURN_EPOCH: AtomicU64 = AtomicU64::new(0);

pub fn start() -> bool {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || unsafe {
        let keyboard =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), std::ptr::null_mut(), 0);
        if keyboard.is_null() || mouse.is_null() {
            if !keyboard.is_null() {
                UnhookWindowsHookEx(keyboard);
            }
            if !mouse.is_null() {
                UnhookWindowsHookEx(mouse);
            }
            let _ = sender.send(false);
            return;
        }
        let _ = sender.send(true);

        let mut message = MSG::default();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(keyboard);
        UnhookWindowsHookEx(mouse);
    });
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or(false)
}

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

    use super::{is_physical_keyboard_event, is_physical_mouse_event};

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
