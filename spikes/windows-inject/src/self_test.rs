use std::error::Error;
use std::io;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, ES_WANTRETURN, GetForegroundWindow, GetMessageW,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow, MSG, PM_REMOVE,
    PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SW_SHOW, SetForegroundWindow,
    SetWindowTextW, ShowWindow, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WM_CLOSE,
    WM_DESTROY, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use crate::diff::plan_transition;
use crate::inject::{send_backspaces, send_text};

const CLASS_NAME: &str = "FlowTypeInjectSpikeWindow";

pub fn run() -> Result<(), Box<dyn Error>> {
    println!("说写 Windows 注入验证 · Win32 自检");
    let receiver = ReceiverWindow::create()?;
    receiver.activate()?;

    let window_value = receiver.window as usize;
    let edit_value = receiver.edit as usize;
    let worker = thread::spawn(move || {
        let result = run_scenarios(window_value as HWND, edit_value as HWND);
        unsafe { PostMessageW(window_value as HWND, WM_CLOSE, 0, 0) };
        result
    });

    receiver.run_message_loop()?;
    worker
        .join()
        .map_err(|_| "Win32 自检线程异常退出")?
        .map_err(Into::into)
}

fn run_scenarios(window: HWND, edit: HWND) -> Result<(), String> {
    let scenarios: &[(&str, &[&str])] = &[
        (
            "unicode",
            &[
                "你好",
                "你好，Windows",
                "你好，Windows 🙂",
                "你好，Windows 🙂 café",
            ],
        ),
        (
            "rewrite",
            &[
                "豆包正在识别",
                "豆包正在识别语音",
                "豆包正在识别文本",
                "豆包语音识别文本。",
            ],
        ),
        (
            "multiline",
            &["第一行", "第一行\n第二行", "第一行\n第二行\n第三行🙂"],
        ),
    ];

    for (name, snapshots) in scenarios {
        clear_edit(edit).map_err(|error| format!("{name}：无法清空控件：{error}"))?;

        let mut previous = "";
        for (step, snapshot) in snapshots.iter().enumerate() {
            if unsafe { GetForegroundWindow() } != window {
                return Err(format!("{name}：自检窗口失去前台，已停止"));
            }
            let transition = plan_transition(previous, snapshot);
            send_backspaces(transition.backspaces)
                .map_err(|error| format!("{name}：退格注入失败：{error}"))?;
            send_text(&transition.insert)
                .map_err(|error| format!("{name}：文字注入失败：{error}"))?;
            thread::sleep(Duration::from_millis(75));

            let expected_step = normalize_newlines(snapshot);
            let actual_step = normalize_newlines(&read_edit_text(edit));
            if actual_step != expected_step {
                return Err(format!(
                    "{name} 步骤 {}：结果不一致；期望 {:?}，实际 {:?}",
                    step + 1,
                    expected_step,
                    actual_step,
                ));
            }
            previous = snapshot;
        }

        let expected = normalize_newlines(snapshots.last().copied().unwrap_or_default());
        let actual = normalize_newlines(&read_edit_text(edit));
        if actual != expected {
            return Err(format!(
                "{name}：注入结果不一致；期望 {} 个字符，实际 {} 个字符",
                expected.chars().count(),
                actual.chars().count(),
            ));
        }
        println!("通过：{name}（{} 个字符）", actual.chars().count());
    }

    println!("Win32 标准编辑控件自检全部通过。");
    Ok(())
}

fn clear_edit(edit: HWND) -> io::Result<()> {
    let empty = wide("");
    if unsafe { SetWindowTextW(edit, empty.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    thread::sleep(Duration::from_millis(50));
    Ok(())
}

fn read_edit_text(edit: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(edit) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0_u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(edit, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

struct ReceiverWindow {
    instance: *mut core::ffi::c_void,
    window: HWND,
    edit: HWND,
    class_name: Vec<u16>,
}

impl ReceiverWindow {
    fn create() -> io::Result<Self> {
        let class_name = wide(CLASS_NAME);
        let window_title = wide("说写 SendInput 自检");
        let edit_class = wide("EDIT");
        let instance = unsafe { GetModuleHandleW(null()) };
        if instance.is_null() {
            return Err(io::Error::last_os_error());
        }

        let window_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(io::Error::last_os_error());
        }

        let window = unsafe {
            CreateWindowExW(
                0 as WINDOW_EX_STYLE,
                class_name.as_ptr(),
                window_title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                120,
                120,
                680,
                360,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            )
        };
        if window.is_null() {
            unsafe { UnregisterClassW(class_name.as_ptr(), instance) };
            return Err(io::Error::last_os_error());
        }

        let edit = unsafe {
            CreateWindowExW(
                0 as WINDOW_EX_STYLE,
                edit_class.as_ptr(),
                null(),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_BORDER
                    | ES_LEFT as u32
                    | ES_MULTILINE as u32
                    | ES_AUTOVSCROLL as u32
                    | ES_WANTRETURN as u32,
                16,
                16,
                630,
                290,
                window,
                null_mut(),
                instance,
                null_mut(),
            )
        };
        if edit.is_null() {
            unsafe {
                DestroyWindow(window);
                UnregisterClassW(class_name.as_ptr(), instance);
            }
            return Err(io::Error::last_os_error());
        }

        unsafe {
            ShowWindow(window, SW_SHOW);
            UpdateWindow(window);
        }

        let receiver = Self {
            instance,
            window,
            edit,
            class_name,
        };
        receiver.pump_messages();
        Ok(receiver)
    }

    fn activate(&self) -> io::Result<()> {
        let foreground = unsafe { GetForegroundWindow() };
        let current_thread = unsafe { GetCurrentThreadId() };
        let foreground_thread = if foreground.is_null() {
            0
        } else {
            unsafe { GetWindowThreadProcessId(foreground, null_mut()) }
        };
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && unsafe { AttachThreadInput(current_thread, foreground_thread, 1) } != 0;

        unsafe {
            ShowWindow(self.window, SW_SHOW);
            BringWindowToTop(self.window);
            SetForegroundWindow(self.window);
            SetFocus(self.edit);
        }
        if attached {
            unsafe { AttachThreadInput(current_thread, foreground_thread, 0) };
        }
        thread::sleep(Duration::from_millis(100));
        self.pump_messages();
        if !self.is_foreground() {
            return Err(io::Error::other("无法把 Win32 自检窗口置于前台"));
        }
        Ok(())
    }

    fn is_foreground(&self) -> bool {
        unsafe { GetForegroundWindow() == self.window }
    }

    fn pump_messages(&self) {
        thread::sleep(Duration::from_millis(25));
        unsafe {
            let mut message: MSG = zeroed();
            while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn run_message_loop(&self) -> io::Result<()> {
        unsafe {
            let mut message: MSG = zeroed();
            loop {
                let result = GetMessageW(&mut message, null_mut(), 0, 0);
                if result == -1 {
                    return Err(io::Error::last_os_error());
                }
                if result == 0 {
                    break;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }
}

impl Drop for ReceiverWindow {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.window) != 0 {
                DestroyWindow(self.window);
            }
            UnregisterClassW(self.class_name.as_ptr(), self.instance);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CLOSE => {
            unsafe { DestroyWindow(window) };
            return 0;
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}
