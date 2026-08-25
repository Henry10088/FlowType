use std::mem::zeroed;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET, DT_CALCRECT, DT_SINGLELINE,
    DeleteObject, DrawTextW, Ellipse, FillRect, GetStockObject, HDC, HFONT, HGDIOBJ, LineTo,
    MoveToEx, PS_SOLID, RoundRect, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, GetWindowTextLengthW, GetWindowTextW, HICON, HMENU, LoadIconW, MF_STRING,
    MessageBoxW,
};

pub(super) fn create_font(pixel_height: i32, weight: i32, face: &str) -> HFONT {
    let face = wide(face);
    unsafe {
        CreateFontW(
            -pixel_height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET.into(),
            0,
            0,
            5,
            0,
            face.as_ptr(),
        )
    }
}

pub(super) fn app_icon() -> HICON {
    unsafe { LoadIconW(GetModuleHandleW(null()), std::ptr::without_provenance(1)) }
}

pub(super) fn fill(dc: HDC, rect: &RECT, color: u32) {
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FillRect(dc, rect, brush);
        DeleteObject(brush);
    }
}

pub(super) fn fill_ellipse(dc: HDC, rect: RECT, color: u32) {
    let brush = unsafe { CreateSolidBrush(color) };
    let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
    let old_brush = unsafe { SelectObject(dc, brush as HGDIOBJ) };
    let old_pen = unsafe { SelectObject(dc, pen as HGDIOBJ) };
    unsafe {
        Ellipse(dc, rect.left, rect.top, rect.right, rect.bottom);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(brush);
        DeleteObject(pen);
    }
}

pub(super) fn outline_round_rect(dc: HDC, rect: RECT, color: u32, radius: i32) {
    let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
    let hollow = unsafe { GetStockObject(5) };
    let old_pen = unsafe { SelectObject(dc, pen as HGDIOBJ) };
    let old_brush = unsafe { SelectObject(dc, hollow) };
    unsafe {
        RoundRect(
            dc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius,
            radius,
        );
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(pen);
    }
}

pub(super) fn draw_line(dc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, color: u32) {
    let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
    let old_pen = unsafe { SelectObject(dc, pen as HGDIOBJ) };
    unsafe {
        MoveToEx(dc, x1, y1, null_mut());
        LineTo(dc, x2, y2);
        SelectObject(dc, old_pen);
        DeleteObject(pen);
    }
}

pub(super) fn draw_label(
    dc: HDC,
    value: &str,
    mut rect: RECT,
    font: HFONT,
    color: u32,
    format: u32,
) {
    let value = wide(value);
    let old_font = unsafe { SelectObject(dc, font as HGDIOBJ) };
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        DrawTextW(dc, value.as_ptr(), -1, &mut rect, format);
        SelectObject(dc, old_font);
    }
}

pub(super) fn measure_text_width(dc: HDC, value: &str, font: HFONT) -> i32 {
    let value = wide(value);
    let mut rect: RECT = unsafe { zeroed() };
    let old_font = unsafe { SelectObject(dc, font as HGDIOBJ) };
    unsafe {
        DrawTextW(
            dc,
            value.as_ptr(),
            -1,
            &mut rect,
            DT_CALCRECT | DT_SINGLELINE,
        );
        SelectObject(dc, old_font);
    }
    rect.right - rect.left
}

pub(super) fn window_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    let mut value = vec![0_u16; length as usize + 1];
    let read = unsafe { GetWindowTextW(hwnd, value.as_mut_ptr(), value.len() as i32) };
    String::from_utf16_lossy(&value[..read as usize])
}

pub(super) fn message_box(hwnd: HWND, text: &str, title: &str, flags: u32) -> i32 {
    let text = wide(text);
    let title = wide(title);
    unsafe { MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), flags) }
}

pub(super) unsafe fn append_menu(menu: HMENU, id: usize, text: &str) {
    let text = wide(text);
    unsafe { AppendMenuW(menu, MF_STRING, id, text.as_ptr()) };
}

pub(super) fn copy_wide<const N: usize>(destination: &mut [u16; N], value: &str) {
    let encoded = value.encode_utf16().take(N - 1).collect::<Vec<_>>();
    destination[..encoded.len()].copy_from_slice(&encoded);
    destination[encoded.len()] = 0;
}

pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
