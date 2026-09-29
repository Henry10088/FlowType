//! Small, shared Win32 components. All geometry is scaled by the caller's DPI.
use super::ui_layout::Rect;
use super::*;
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};

pub(super) const CORNER_RADIUS: i32 = 8;
pub(super) const NAV_INSET: i32 = 10;

// One set of section bounds is shared by Direct2D and the GDI recovery renderer.
pub(super) fn page_panels(page: Page, client_width: f32, phone_count: usize) -> Vec<Rect> {
    let right = ShellLayout::new(client_width).content_right;
    let panel = |top, bottom| Rect {
        left: 204.0,
        top,
        right,
        bottom,
    };
    match page {
        Page::Status => vec![panel(92.0, 296.0)],
        Page::Phones => PhonesLayout::new(client_width, phone_count)
            .rows
            .iter()
            .map(|row| Rect {
                left: 210.0,
                top: row.bounds.top + 2.0,
                right,
                bottom: row.bounds.bottom - 2.0,
            })
            .collect(),
        Page::Pairing => vec![panel(108.0, 426.0)],
        Page::Files => vec![panel(88.0, 388.0), panel(402.0, 520.0)],
        Page::Settings => vec![panel(84.0, 254.0), panel(266.0, 360.0), panel(364.0, 552.0)],
    }
}

pub(super) fn device_rect(rect: Rect, dpi: u32) -> RECT {
    let scale = |value: f32| (value * dpi as f32 / 96.0).round() as i32;
    RECT {
        left: scale(rect.left),
        top: scale(rect.top),
        right: scale(rect.right),
        bottom: scale(rect.bottom),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionStyle {
    Primary,
    Secondary,
    Subtle,
}

pub(super) fn draw_action(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    style: ActionStyle,
    state: ButtonVisual,
    radius: i32,
) {
    let background = match (style, state.disabled, state.pressed, state.hovered) {
        (ActionStyle::Primary, true, _, _) => COLOR_DISABLED,
        (ActionStyle::Primary, false, true, _) => COLOR_TEAL_PRESSED,
        (ActionStyle::Primary, false, false, true) => COLOR_TEAL_HOVER,
        (ActionStyle::Primary, false, false, false) => COLOR_TEAL,
        (_, true, _, _) => COLOR_SIDEBAR,
        (_, false, true, _) => COLOR_PRESSED,
        (_, false, false, true) => COLOR_HOVER,
        (ActionStyle::Secondary, false, false, false) => COLOR_WHITE,
        (ActionStyle::Subtle, false, false, false) => COLOR_SIDEBAR,
    };
    fill_round_rect(dc, rect, background, radius);
    if style == ActionStyle::Secondary {
        outline_round_rect(dc, rect, COLOR_LINE, radius * 2);
    }
    if state.focused {
        let inset = (radius / 3).max(2);
        outline_round_rect(
            dc,
            RECT {
                left: rect.left + inset,
                top: rect.top + inset,
                right: rect.right - inset,
                bottom: rect.bottom - inset,
            },
            COLOR_TEAL,
            radius * 2,
        );
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct ButtonVisual {
    pub pressed: bool,
    pub hovered: bool,
    pub focused: bool,
    pub disabled: bool,
}

pub(super) fn action_text_color(style: ActionStyle, disabled: bool) -> u32 {
    if disabled {
        COLOR_MUTED
    } else if style == ActionStyle::Primary {
        COLOR_WHITE
    } else {
        COLOR_TEXT
    }
}

pub(super) fn draw_panel(dc: windows_sys::Win32::Graphics::Gdi::HDC, rect: RECT, radius: i32) {
    fill_round_rect(dc, rect, COLOR_WHITE, radius);
    outline_round_rect(dc, rect, COLOR_LINE, radius * 2);
}

pub(super) fn track_button_hover(button: HWND) {
    unsafe { SetWindowSubclass(button, Some(button_subclass), 1, 0) };
}

pub(super) fn button_hovered(button: HWND) -> bool {
    unsafe { GetWindowLongPtrW(button, GWLP_USERDATA) != 0 }
}

unsafe extern "system" fn button_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _data: usize,
) -> LRESULT {
    match message {
        WM_MOUSEMOVE if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 0 => {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 1) };
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut track) };
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        WM_MOUSELEAVE => {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        WM_NCDESTROY => {
            unsafe { RemoveWindowSubclass(hwnd, Some(button_subclass), 1) };
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_bounds_fit_the_minimum_client_width() {
        for page in [
            Page::Status,
            Page::Files,
            Page::Phones,
            Page::Settings,
            Page::Pairing,
        ] {
            for rect in page_panels(page, 680.0, 2) {
                assert!(rect.left < rect.right && rect.right <= 680.0);
                assert!(rect.top < rect.bottom);
            }
        }
    }
}
