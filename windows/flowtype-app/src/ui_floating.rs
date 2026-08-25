use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::{Arc, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
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
use crate::ui::ui_paint::wide;
use crate::ui::ui_theme::{COLOR_BALL_BLACK, COLOR_BALL_ORANGE};

const CLASS_NAME: &str = "FlowTypeFloatingBall";
const CLICK_TIMER: usize = 1;
const BALL_SIZE: i32 = 48;
const SHADOW_MARGIN: i32 = 7;
const BALL_MARGIN: i32 = 24;
const BALL_BORDER: (u8, u8, u8) = (255, 255, 255);

#[derive(Clone, Copy)]
struct Paint {
    color: (u8, u8, u8),
    alpha: f32,
}

struct LayeredSurface {
    dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u8,
    size: i32,
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old_bitmap);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

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
    ball_size: i32,
    shadow_margin: i32,
    size: i32,
    surface: Option<LayeredSurface>,
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
    let ball_size = BALL_SIZE * scale / 96;
    let shadow_margin = SHADOW_MARGIN * scale / 96;
    let size = ball_size + shadow_margin * 2;
    let (x, y) = settings::floating_position()
        .map(|(x, y)| (x - shadow_margin, y - shadow_margin))
        .unwrap_or_else(|| default_position(size));
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
        ball_size,
        shadow_margin,
        size,
        surface: None,
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
    render_surface(context);
    update_layered_window(context);
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

fn ball_color(context: &BallContext) -> (u8, u8, u8) {
    let snapshot = context.state.snapshot();
    if snapshot.status.connected_phone.is_some() {
        // A lighter green keeps the translucent ball readable over bright windows.
        return (54, 201, 198);
    };
    let color = if snapshot.phones.is_empty() {
        COLOR_BALL_BLACK
    } else {
        COLOR_BALL_ORANGE
    };
    (
        (color & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        ((color >> 16) & 0xff) as u8,
    )
}

fn status_dot_color(context: &BallContext) -> (u8, u8, u8) {
    let snapshot = context.state.snapshot();
    if snapshot.phones.is_empty() {
        (145, 154, 159)
    } else if snapshot.status.connected_phone.is_some() {
        (244, 246, 247)
    } else {
        (255, 199, 111)
    }
}

fn create_surface(size: i32) -> Option<LayeredSurface> {
    let dc = unsafe { CreateCompatibleDC(null_mut()) };
    if dc.is_null() {
        return None;
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..unsafe { zeroed() }
        },
        ..unsafe { zeroed() }
    };
    let mut bits = null_mut();
    let bitmap = unsafe { CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        unsafe { DeleteDC(dc) };
        return None;
    }
    let old_bitmap = unsafe { SelectObject(dc, bitmap as HGDIOBJ) };
    Some(LayeredSurface {
        dc,
        bitmap,
        old_bitmap,
        bits: bits.cast(),
        size,
    })
}

fn render_surface(context: &mut BallContext) {
    if context.surface.is_none() {
        context.surface = create_surface(context.size);
    }
    let Some(surface) = context.surface.as_mut() else {
        return;
    };
    let pixel_count = (surface.size * surface.size) as usize;
    let pixels = unsafe { std::slice::from_raw_parts_mut(surface.bits, pixel_count * 4) };
    pixels.fill(0);

    let center = context.size as f32 / 2.0;
    let radius = context.ball_size as f32 / 2.0;
    draw_soft_circle(
        pixels,
        context.size as usize,
        center,
        center + 1.4,
        radius,
        context.shadow_margin as f32 * 1.5,
        Paint {
            color: (0, 0, 0),
            alpha: 0.42,
        },
    );

    let color = ball_color(context);
    draw_circle(
        pixels,
        context.size as usize,
        center,
        center,
        radius,
        Paint { color, alpha: 0.45 },
    );
    draw_ring(
        pixels,
        context.size as usize,
        center,
        center,
        radius - 0.25,
        1.1,
        Paint {
            color: BALL_BORDER,
            alpha: 0.2,
        },
    );

    // Match the Android drawable's 48x48 view box rather than the old 56px ball.
    let icon_size = (context.ball_size * 28 / 56).max(1);
    draw_android_edit_icon(pixels, context.size as usize, center, icon_size);

    let dot_scale = context.ball_size as f32 / 56.0;
    let dot_center = (center + radius - 10.5, center + radius - 10.5);
    draw_circle(
        pixels,
        context.size as usize,
        dot_center.0,
        dot_center.1,
        4.7 * dot_scale,
        Paint {
            color: status_dot_color(context),
            alpha: 1.0,
        },
    );
    draw_ring(
        pixels,
        context.size as usize,
        dot_center.0,
        dot_center.1,
        5.2 * dot_scale,
        1.3,
        Paint {
            color: (15, 28, 30),
            alpha: 0.45,
        },
    );
}

fn update_layered_window(context: &BallContext) {
    let Some(surface) = context.surface.as_ref() else {
        return;
    };
    let mut window: RECT = unsafe { zeroed() };
    unsafe { GetWindowRect(context.hwnd, &mut window) };
    let destination = POINT {
        x: window.left,
        y: window.top,
    };
    let size = SIZE {
        cx: context.size,
        cy: context.size,
    };
    let source = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    unsafe {
        UpdateLayeredWindow(
            context.hwnd,
            null_mut(),
            &destination,
            &size,
            surface.dc,
            &source,
            0,
            &blend,
            ULW_ALPHA,
        );
    }
}

fn blend_pixel(pixels: &mut [u8], width: usize, x: i32, y: i32, color: (u8, u8, u8), alpha: f32) {
    if x < 0 || y < 0 || x as usize >= width || y as usize >= width || alpha <= 0.0 {
        return;
    }
    let alpha = alpha.min(1.0);
    let index = ((y as usize * width) + x as usize) * 4;
    let destination_alpha = pixels[index + 3] as f32 / 255.0;
    let output_alpha = alpha + destination_alpha * (1.0 - alpha);
    if output_alpha <= 0.0 {
        return;
    }
    let source = [
        color.2 as f32 / 255.0 * alpha,
        color.1 as f32 / 255.0 * alpha,
        color.0 as f32 / 255.0 * alpha,
    ];
    pixels[index] =
        ((source[0] + pixels[index] as f32 / 255.0 * (1.0 - alpha)) * 255.0).round() as u8;
    pixels[index + 1] =
        ((source[1] + pixels[index + 1] as f32 / 255.0 * (1.0 - alpha)) * 255.0).round() as u8;
    pixels[index + 2] =
        ((source[2] + pixels[index + 2] as f32 / 255.0 * (1.0 - alpha)) * 255.0).round() as u8;
    pixels[index + 3] = (output_alpha * 255.0).round() as u8;
}

fn draw_circle(pixels: &mut [u8], width: usize, cx: f32, cy: f32, radius: f32, paint: Paint) {
    let min_x = (cx - radius - 1.0).floor() as i32;
    let max_x = (cx + radius + 1.0).ceil() as i32;
    let min_y = (cy - radius - 1.0).floor() as i32;
    let max_y = (cy + radius + 1.0).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let coverage = (radius + 0.65 - distance).clamp(0.0, 1.0);
            blend_pixel(pixels, width, x, y, paint.color, paint.alpha * coverage);
        }
    }
}

fn draw_soft_circle(
    pixels: &mut [u8],
    width: usize,
    cx: f32,
    cy: f32,
    radius: f32,
    blur: f32,
    paint: Paint,
) {
    let outer = radius + blur;
    let min_x = (cx - outer - 1.0).floor() as i32;
    let max_x = (cx + outer + 1.0).ceil() as i32;
    let min_y = (cy - outer - 1.0).floor() as i32;
    let max_y = (cy + outer + 1.0).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if distance <= radius {
                continue;
            }
            let coverage = (1.0 - (distance - radius) / blur).max(0.0).powi(2);
            blend_pixel(pixels, width, x, y, paint.color, paint.alpha * coverage);
        }
    }
}

fn draw_ring(
    pixels: &mut [u8],
    width: usize,
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    paint: Paint,
) {
    let outer = radius + thickness / 2.0;
    let inner = radius - thickness / 2.0;
    let min_x = (cx - outer - 1.0).floor() as i32;
    let max_x = (cx + outer + 1.0).ceil() as i32;
    let min_y = (cy - outer - 1.0).floor() as i32;
    let max_y = (cy + outer + 1.0).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let outer_coverage = (outer + 0.65 - distance).clamp(0.0, 1.0);
            let inner_coverage = (distance - inner + 0.65).clamp(0.0, 1.0);
            blend_pixel(
                pixels,
                width,
                x,
                y,
                paint.color,
                paint.alpha * outer_coverage.min(inner_coverage),
            );
        }
    }
}

fn draw_android_edit_icon(pixels: &mut [u8], width: usize, center: f32, size: i32) {
    const SOURCE_SIZE: usize = 32;
    static ICON_PIXELS: OnceLock<Vec<u8>> = OnceLock::new();
    let source = ICON_PIXELS.get_or_init(|| {
        image::load_from_memory(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/ic_menu_edit.png"
        )))
        .expect("embedded Android edit icon must decode")
        .to_rgba8()
        .into_raw()
    });
    let left = (center - size as f32 / 2.0).floor() as i32;
    let top = (center - size as f32 / 2.0).floor() as i32;
    for y in 0..size {
        for x in 0..size {
            let source_x = (((x as f32 + 0.5) * SOURCE_SIZE as f32 / size as f32) - 0.5)
                .round()
                .clamp(0.0, (SOURCE_SIZE - 1) as f32) as usize;
            let source_y = (((y as f32 + 0.5) * SOURCE_SIZE as f32 / size as f32) - 0.5)
                .round()
                .clamp(0.0, (SOURCE_SIZE - 1) as f32) as usize;
            let alpha = source[(source_y * SOURCE_SIZE + source_x) * 4 + 3] as f32 / 255.0;
            blend_pixel(pixels, width, left + x, top + y, (244, 246, 247), alpha);
        }
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
            create_tooltip(context);
            refresh(hwnd);
            0
        }
        WM_NCHITTEST => {
            let mut cursor = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut cursor) };
            let mut window: RECT = unsafe { zeroed() };
            unsafe { GetWindowRect(hwnd, &mut window) };
            let x = cursor.x - window.left;
            let y = cursor.y - window.top;
            let center = context.size as f32 / 2.0;
            let distance =
                ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
            if distance <= context.ball_size as f32 / 2.0 + 1.0 {
                HTCLIENT as LRESULT
            } else {
                HTTRANSPARENT as LRESULT
            }
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
                let _ = settings::set_floating_position((
                    window.left + context.shadow_margin,
                    window.top + context.shadow_margin,
                ));
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
        WM_ERASEBKGND | WM_PAINT => 0,
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
