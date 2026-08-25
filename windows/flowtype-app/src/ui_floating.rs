use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateEllipticRgn, EndPaint, InvalidateRect, PAINTSTRUCT, SetWindowRgn,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX,
    TTTOOLINFOW,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetDoubleClickTime, ReleaseCapture, SetCapture,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::AppState;
use crate::settings;
use crate::ui::ui_paint::{draw_brand_mark, fill, wide};
use crate::ui::ui_theme::{COLOR_BALL_BLACK, COLOR_BALL_ORANGE, COLOR_TEAL};

const CLASS_NAME: &str = "FlowTypeFloatingBall";
const CLICK_TIMER: usize = 1;
const BALL_SIZE: i32 = 56;
const BALL_MARGIN: i32 = 24;

struct BallContext {
    state: Arc<AppState>,
    main_hwnd: HWND,
    hwnd: HWND,
    tooltip: HWND,
    tooltip_text: Vec<u16>,
    drag_origin: POINT,
    window_origin: POINT,
    dragging: bool,
    pending_click: bool,
    size: i32,
}

pub fn create(state: Arc<AppState>, main_hwnd: HWND) -> HWND {
    let instance = unsafe { GetModuleHandleW(null()) };
    let class_name = wide(CLASS_NAME);
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_HAND) },
        lpszClassName: class_name.as_ptr(),
        ..unsafe { zeroed() }
    };
    unsafe { RegisterClassW(&class) };

    let scale = unsafe { GetDpiForSystem() }.max(96) as i32;
    let size = BALL_SIZE * scale / 96;
    let (x, y) = settings::floating_position().unwrap_or_else(|| default_position(size));
    let context = Box::new(BallContext {
        state,
        main_hwnd,
        hwnd: null_mut(),
        tooltip: null_mut(),
        tooltip_text: Vec::new(),
        drag_origin: POINT { x: 0, y: 0 },
        window_origin: POINT { x, y },
        dragging: false,
        pending_click: false,
        size,
    });
    let context_ptr = Box::into_raw(context);
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            class_name.as_ptr(),
            wide("说写").as_ptr(),
            WS_POPUP,
            x,
            y,
            size,
            size,
            null_mut(),
            null_mut(),
            instance,
            context_ptr.cast(),
        )
    };
    if hwnd.is_null() {
        unsafe { drop(Box::from_raw(context_ptr)) };
        return null_mut();
    }
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    hwnd
}

pub fn refresh(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let context = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BallContext };
    if context.is_null() {
        return;
    }
    let context = unsafe { &mut *context };
    update_tooltip(context);
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

fn default_position(size: i32) -> (i32, i32) {
    let mut work: RECT = unsafe { zeroed() };
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
    }
    (
        work.right - size - BALL_MARGIN,
        work.bottom - size - BALL_MARGIN,
    )
}

fn update_tooltip(context: &mut BallContext) {
    let snapshot = context.state.snapshot();
    let text = if snapshot.phones.is_empty() {
        "说写\n尚未绑定手机\n双击打开主页面".to_owned()
    } else if let Some(phone) = snapshot.status.connected_phone.as_deref() {
        format!("说写\n已连接：{phone}\n单击切换到此电脑\n双击打开主页面")
    } else {
        "说写\n手机连接已断开\n双击打开主页面".to_owned()
    };
    context.tooltip_text = wide(&text);
    if context.tooltip.is_null() {
        return;
    }
    let mut tool = tooltip_info(context);
    unsafe {
        SendMessageW(
            context.tooltip,
            TTM_UPDATETIPTEXTW,
            0,
            (&mut tool as *mut TTTOOLINFOW) as LPARAM,
        );
    }
}

fn tooltip_info(context: &mut BallContext) -> TTTOOLINFOW {
    TTTOOLINFOW {
        cbSize: size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
        hwnd: context.hwnd,
        uId: context.hwnd as usize,
        lpszText: context.tooltip_text.as_mut_ptr(),
        ..unsafe { zeroed() }
    }
}

fn create_tooltip(context: &mut BallContext) {
    let class_name = wide("tooltips_class32");
    let tooltip = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            class_name.as_ptr(),
            null(),
            WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
            0,
            0,
            0,
            0,
            context.hwnd,
            null_mut(),
            GetModuleHandleW(null()),
            null_mut(),
        )
    };
    if tooltip.is_null() {
        return;
    }
    context.tooltip = tooltip;
    update_tooltip(context);
    let mut tool = tooltip_info(context);
    unsafe {
        SendMessageW(
            tooltip,
            TTM_ADDTOOLW,
            0,
            (&mut tool as *mut TTTOOLINFOW) as LPARAM,
        );
    }
}

fn ball_color(context: &BallContext) -> u32 {
    let snapshot = context.state.snapshot();
    if snapshot.phones.is_empty() {
        COLOR_BALL_BLACK
    } else if snapshot.status.connected_phone.is_some() {
        COLOR_TEAL
    } else {
        COLOR_BALL_ORANGE
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        let context = create.lpCreateParams as *mut BallContext;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, context as isize);
            (*context).hwnd = hwnd;
        }
    }
    let context = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BallContext };
    if context.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let context = unsafe { &mut *context };
    match message {
        WM_CREATE => {
            let region = unsafe { CreateEllipticRgn(0, 0, context.size, context.size) };
            unsafe {
                SetWindowRgn(hwnd, region, 1);
                SetLayeredWindowAttributes(hwnd, 0, 224, LWA_ALPHA);
            }
            create_tooltip(context);
            refresh(hwnd);
            0
        }
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { zeroed() };
            let dc = unsafe { BeginPaint(hwnd, &mut paint) };
            let rect = RECT {
                left: 1,
                top: 1,
                right: context.size - 1,
                bottom: context.size - 1,
            };
            fill(dc, &rect, ball_color(context));
            draw_brand_mark(
                dc,
                RECT {
                    left: context.size * 13 / 56,
                    top: context.size * 13 / 56,
                    right: context.size * 43 / 56,
                    bottom: context.size * 43 / 56,
                },
            );
            unsafe { EndPaint(hwnd, &paint) };
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_LBUTTONDOWN => {
            let mut cursor = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut cursor) };
            let mut window: RECT = unsafe { zeroed() };
            unsafe { GetWindowRect(hwnd, &mut window) };
            context.drag_origin = cursor;
            context.window_origin = POINT {
                x: window.left,
                y: window.top,
            };
            context.dragging = false;
            unsafe { SetCapture(hwnd) };
            0
        }
        WM_MOUSEMOVE if unsafe { GetCapture() } == hwnd => {
            let mut cursor = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut cursor) };
            let dx = cursor.x - context.drag_origin.x;
            let dy = cursor.y - context.drag_origin.y;
            let threshold = unsafe { GetSystemMetrics(SM_CXDRAG).max(4) };
            if !context.dragging && (dx.abs() >= threshold || dy.abs() >= threshold) {
                context.dragging = true;
                context.pending_click = false;
                unsafe { KillTimer(hwnd, CLICK_TIMER) };
            }
            if context.dragging {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        context.window_origin.x + dx,
                        context.window_origin.y + dy,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
            0
        }
        WM_LBUTTONUP => {
            unsafe { ReleaseCapture() };
            if context.dragging {
                let mut window: RECT = unsafe { zeroed() };
                unsafe { GetWindowRect(hwnd, &mut window) };
                let _ = settings::set_floating_position((window.left, window.top));
            } else {
                context.pending_click = true;
                unsafe { SetTimer(hwnd, CLICK_TIMER, GetDoubleClickTime(), None) };
            }
            0
        }
        WM_LBUTTONDBLCLK => {
            context.pending_click = false;
            unsafe { KillTimer(hwnd, CLICK_TIMER) };
            unsafe { PostMessageW(context.main_hwnd, super::WM_APP_SHOW, 0, 0) };
            0
        }
        WM_TIMER if wparam == CLICK_TIMER => {
            unsafe { KillTimer(hwnd, CLICK_TIMER) };
            if context.pending_click {
                context.pending_click = false;
                context.state.request_switch_to_current();
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_NCDESTROY => {
            if !context.tooltip.is_null() {
                unsafe { DestroyWindow(context.tooltip) };
            }
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(context));
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
