use std::ffi::c_void;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetWindow(HWND);

impl TargetWindow {
    pub fn capture_foreground() -> Option<Self> {
        let handle = unsafe { GetForegroundWindow() };
        if handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }

    pub fn is_foreground(self) -> bool {
        unsafe { GetForegroundWindow() == self.0 }
    }

    pub fn title(self) -> String {
        let length = unsafe { GetWindowTextLengthW(self.0) };
        if length <= 0 {
            return "（无标题）".to_owned();
        }

        let mut buffer = vec![0_u16; length as usize + 1];
        let copied = unsafe { GetWindowTextW(self.0, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }

    pub fn raw_value(self) -> usize {
        self.0.cast::<c_void>() as usize
    }
}
