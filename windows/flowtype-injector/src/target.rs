use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GW_HWNDNEXT, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetForegroundWindow,
};

#[derive(Clone, Copy)]
pub struct TargetWindow {
    handle: HWND,
    process_id: u32,
}

impl TargetWindow {
    pub fn capture_foreground() -> Option<Self> {
        let foreground = unsafe { GetForegroundWindow() };
        let current = Self::from_handle(foreground)?;
        if !current.is_flowtype_window() {
            return Some(current);
        }

        // The main window is often left open above the editor while input is
        // started from the phone. Use the nearest visible external window in
        // the z-order instead of treating FlowType's own UI as the target.
        let mut candidate = unsafe { GetWindow(foreground, GW_HWNDNEXT) };
        while !candidate.is_null() {
            if unsafe { IsWindowVisible(candidate) } != 0
                && let Some(target) = Self::from_handle(candidate)
                && !target.is_flowtype_window()
            {
                return Some(target);
            }
            candidate = unsafe { GetWindow(candidate, GW_HWNDNEXT) };
        }
        None
    }

    pub fn is_foreground(self) -> bool {
        let foreground = unsafe { GetForegroundWindow() };
        foreground == self.handle
            || Self::from_handle(foreground).is_some_and(|window| window.is_flowtype_window())
    }

    pub fn is_valid(self) -> bool {
        if unsafe { IsWindow(self.handle) } == 0 {
            return false;
        }
        let mut current_process_id = 0;
        unsafe { GetWindowThreadProcessId(self.handle, &mut current_process_id) };
        current_process_id == self.process_id
    }

    /// A non-fullscreen RDP client is a host-side container for another PC.
    /// Its mouse activity updates the host's global input clock, but it is not
    /// a meaningful local text target for FlowType.
    pub fn is_remote_desktop_client(self) -> bool {
        if is_remote_desktop_process_name(&self.process_name()) {
            return true;
        }
        let mut buffer = [0_u16; 256];
        let length =
            unsafe { GetClassNameW(self.handle, buffer.as_mut_ptr(), buffer.len() as i32) };
        let class_name = String::from_utf16_lossy(&buffer[..length.max(0) as usize]);
        matches!(
            class_name.to_ascii_lowercase().as_str(),
            "tscshellcontainerclass" | "rail_window" | "msrdcwindow"
        )
    }

    pub fn is_flowtype_window(self) -> bool {
        self.process_name().eq_ignore_ascii_case("flowtype.exe")
    }

    pub fn activate_for_input(self) -> bool {
        if unsafe { GetForegroundWindow() } == self.handle {
            return true;
        }
        let foreground = unsafe { GetForegroundWindow() };
        let foreground_thread = if foreground.is_null() {
            0
        } else {
            unsafe { GetWindowThreadProcessId(foreground, std::ptr::null_mut()) }
        };
        let target_thread = self.thread_id();
        let current_thread = unsafe { GetCurrentThreadId() };
        let mut attached_foreground = false;
        let mut attached_target = false;
        if foreground_thread != 0 && foreground_thread != current_thread {
            attached_foreground =
                unsafe { AttachThreadInput(current_thread, foreground_thread, 1) } != 0;
        }
        if target_thread != 0 && target_thread != current_thread {
            attached_target = unsafe { AttachThreadInput(current_thread, target_thread, 1) } != 0;
        }
        let activated = unsafe { SetForegroundWindow(self.handle) } != 0;
        if attached_target {
            unsafe { AttachThreadInput(current_thread, target_thread, 0) };
        }
        if attached_foreground {
            unsafe { AttachThreadInput(current_thread, foreground_thread, 0) };
        }
        activated && unsafe { GetForegroundWindow() } == self.handle
    }

    pub fn title(self) -> String {
        let length = unsafe { GetWindowTextLengthW(self.handle) };
        if length <= 0 {
            return "当前窗口".to_owned();
        }
        let mut buffer = vec![0_u16; length as usize + 1];
        let copied =
            unsafe { GetWindowTextW(self.handle, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }

    pub fn process_id(self) -> u32 {
        self.process_id
    }

    pub fn thread_id(self) -> u32 {
        unsafe { GetWindowThreadProcessId(self.handle, std::ptr::null_mut()) }
    }

    pub fn activity_age_ms(self) -> u64 {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            // Keep the value representable by Android's signed Long JSON parser.
            return i64::MAX as u64;
        }
        u64::from(unsafe { GetTickCount().wrapping_sub(info.dwTime) })
    }

    fn process_name(self) -> String {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, self.process_id) };
        if process.is_null() {
            return String::new();
        }
        let mut buffer = vec![0_u16; 32_768];
        let mut length = buffer.len() as u32;
        let queried =
            unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
        unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
        if queried == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..length as usize])
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    fn from_handle(handle: HWND) -> Option<Self> {
        if handle.is_null() {
            return None;
        }
        let mut process_id = 0;
        unsafe { GetWindowThreadProcessId(handle, &mut process_id) };
        (process_id != 0).then_some(Self { handle, process_id })
    }
}

fn is_remote_desktop_process_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "mstsc.exe" | "msrdc.exe" | "rdclient.exe" | "windows365.exe"
    )
}

#[cfg(test)]
mod tests {
    use super::is_remote_desktop_process_name;

    #[test]
    fn recognizes_common_remote_desktop_clients() {
        assert!(is_remote_desktop_process_name("mstsc.exe"));
        assert!(is_remote_desktop_process_name("MsRdc.exe"));
        assert!(is_remote_desktop_process_name("rdclient.exe"));
        assert!(!is_remote_desktop_process_name("notepad++.exe"));
    }
}
