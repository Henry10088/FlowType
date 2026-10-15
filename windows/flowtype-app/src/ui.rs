use std::mem::{size_of, zeroed};
use std::net::IpAddr;
use std::ptr::{null, null_mut};
use std::sync::Arc;

use qrcode::{Color, QrCode};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DT_CENTER,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, EndPaint, FW_NORMAL, FW_SEMIBOLD,
    FillRect, GetStockObject, HFONT, HGDIOBJ, InvalidateRect, PAINTSTRUCT, RDW_ALLCHILDREN,
    RDW_ERASE, RDW_INVALIDATE, RDW_NOERASE, RDW_UPDATENOW, RedrawWindow, SRCCOPY, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT, UpdateWindow, WHITE_BRUSH,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemServices::SS_NOPREFIX;
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED, PBM_SETPOS, PBM_SETRANGE32,
    PBS_SMOOTH, TOOLTIPS_CLASSW, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTS_ALWAYSTIP,
    TTS_NOPREFIX, TTTOOLINFOW,
};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::Shell::{
    DragAcceptFiles, DragFinish, DragQueryFileW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP,
    NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::i18n::{
    LanguageChoice, is_chinese, language_choice, product_name, set_language_choice, tr,
};
use crate::{AppState, PORT, WM_APP_STATE, settings, update};

#[path = "ui_actions.rs"]
mod ui_actions;
#[path = "ui_commands.rs"]
mod ui_commands;
#[path = "ui_components.rs"]
mod ui_components;
#[path = "ui_direct2d.rs"]
mod ui_direct2d;
#[path = "ui_floating.rs"]
mod ui_floating;
#[path = "ui_layout.rs"]
mod ui_layout;
#[path = "ui_pages.rs"]
mod ui_pages;
#[path = "ui_paint.rs"]
mod ui_paint;
#[path = "ui_render.rs"]
mod ui_render;
#[path = "ui_state.rs"]
mod ui_state;
#[path = "ui_theme.rs"]
mod ui_theme;
#[path = "ui_tray.rs"]
mod ui_tray;

use ui_commands::*;
use ui_components::*;
use ui_direct2d::{Direct2dPainter, PhonePaintRow};
use ui_layout::{
    FilesLayout, MIN_CLIENT_HEIGHT, MIN_CLIENT_WIDTH, PairingLayout, PhonesLayout, SettingsLayout,
    ShellLayout, StatusLayout,
};
use ui_paint::*;
use ui_state::FileTabContent;
use ui_theme::*;

const CLASS_NAME: &str = "FlowTypeMainWindow";
const WM_APP_TRAY: u32 = 0x8002;
const WM_APP_SHOW: u32 = 0x8003;
const WM_APP_FLOATING_HIDE: u32 = 0x8004;
const TRAY_ID: u32 = 1;
const NOTICE_TIMER: usize = 2;
const ID_LANGUAGE_SYSTEM: usize = 4101;
const ID_LANGUAGE_CHINESE: usize = 4102;
const ID_LANGUAGE_ENGLISH: usize = 4103;

struct SaveNotice {
    success: bool,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Status,
    Files,
    Phones,
    Settings,
    Pairing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileTab {
    Send,
    Received,
}

#[derive(Clone, Copy)]
enum ButtonKind {
    Navigation(Page),
    Secondary,
    Back,
    Checkbox,
    Language,
    WindowMinimize,
    WindowClose,
    FileTab(FileTab),
}

struct UiContext {
    state: Arc<AppState>,
    host: IpAddr,
    hwnd: HWND,
    page: Page,
    file_tab: FileTab,
    shell_controls: Vec<HWND>,
    controls: Vec<HWND>,
    buttons: Vec<(HWND, ButtonKind)>,
    muted_controls: Vec<HWND>,
    phone_ids: Vec<String>,
    qr: Option<(usize, Vec<bool>)>,
    name_edit: HWND,
    status_summary: HWND,
    status_phone: HWND,
    status_input: HWND,
    status_target: HWND,
    status_address: HWND,
    status_error: HWND,
    phone_statuses: Vec<HWND>,
    phone_names: Vec<HWND>,
    file_destination: HWND,
    file_status: HWND,
    file_failures: HWND,
    file_drop_hint: HWND,
    file_security_note: HWND,
    file_receive_root: HWND,
    file_choose: HWND,
    file_folder: HWND,
    file_cancel: HWND,
    file_cancel_batch: Option<String>,
    pair_open_instruction: HWND,
    pair_scan_instruction: HWND,
    pair_computer: HWND,
    pair_waiting: HWND,
    pair_note: HWND,
    service_status: HWND,
    auto_start: HWND,
    show_floating: HWND,
    language_button: HWND,
    language_tooltip_text: Vec<u16>,
    update_status: HWND,
    update_progress: HWND,
    update_action: HWND,
    update_repository: HWND,
    update_history: HWND,
    title_font: HFONT,
    heading_font: HFONT,
    navigation_font: HFONT,
    body_font: HFONT,
    icon_font: HFONT,
    auto_start_checked: bool,
    floating_enabled_checked: bool,
    ball_hwnd: HWND,
    save_notice: Option<SaveNotice>,
    direct2d: Option<Direct2dPainter>,
    #[cfg(debug_assertions)]
    simulate_paint_error: bool,
    last_transfer_status: Option<(String, flowtype_core::file_transfer::BatchState)>,
}

impl UiContext {
    fn new(state: Arc<AppState>, host: IpAddr, page: Page) -> Self {
        let last_transfer_status = state
            .snapshot()
            .file_transfer
            .map(|transfer| (transfer.batch_id, transfer.state));
        Self {
            state,
            host,
            hwnd: null_mut(),
            page,
            file_tab: FileTab::Send,
            shell_controls: Vec::new(),
            controls: Vec::new(),
            buttons: Vec::new(),
            muted_controls: Vec::new(),
            phone_ids: Vec::new(),
            qr: None,
            name_edit: null_mut(),
            status_summary: null_mut(),
            status_phone: null_mut(),
            status_input: null_mut(),
            status_target: null_mut(),
            status_address: null_mut(),
            status_error: null_mut(),
            phone_statuses: Vec::new(),
            phone_names: Vec::new(),
            file_destination: null_mut(),
            file_status: null_mut(),
            file_failures: null_mut(),
            file_drop_hint: null_mut(),
            file_security_note: null_mut(),
            file_receive_root: null_mut(),
            file_choose: null_mut(),
            file_folder: null_mut(),
            file_cancel: null_mut(),
            file_cancel_batch: None,
            pair_open_instruction: null_mut(),
            pair_scan_instruction: null_mut(),
            pair_computer: null_mut(),
            pair_waiting: null_mut(),
            pair_note: null_mut(),
            service_status: null_mut(),
            auto_start: null_mut(),
            show_floating: null_mut(),
            language_button: null_mut(),
            language_tooltip_text: Vec::new(),
            update_status: null_mut(),
            update_progress: null_mut(),
            update_action: null_mut(),
            update_repository: null_mut(),
            update_history: null_mut(),
            title_font: null_mut(),
            heading_font: null_mut(),
            navigation_font: null_mut(),
            body_font: null_mut(),
            icon_font: null_mut(),
            auto_start_checked: false,
            floating_enabled_checked: settings::floating_enabled(),
            ball_hwnd: null_mut(),
            save_notice: None,
            direct2d: None,
            #[cfg(debug_assertions)]
            simulate_paint_error: std::env::var_os("FLOWTYPE_UI_SIMULATE_PAINT_FAILURE").is_some(),
            last_transfer_status,
        }
    }

    fn scale(&self, value: i32) -> i32 {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        value * dpi as i32 / 96
    }

    fn client_size(&self) -> (i32, i32) {
        let mut client: RECT = unsafe { zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut client) };
        (client.right.max(1), client.bottom.max(1))
    }

    fn ensure_painter(&mut self) {
        if self.direct2d.is_some() || std::env::var_os("FLOWTYPE_UI_FORCE_GDI").is_some() {
            return;
        }
        let (width, height) = self.client_size();
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32;
        self.direct2d = Direct2dPainter::new(self.hwnd, width as u32, height as u32, dpi)
            .map_err(|error| crate::diagnostics::log(format!("UI target create: {error}")))
            .ok();
    }
}

pub fn run(
    state: Arc<AppState>,
    host: IpAddr,
    visible: bool,
    pairing_preview: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES | ICC_PROGRESS_CLASS,
    };
    unsafe { InitCommonControlsEx(&controls) };
    let instance = unsafe { GetModuleHandleW(null()) };
    let class_name = wide(CLASS_NAME);
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hIcon: app_icon(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: null_mut(),
        lpszClassName: class_name.as_ptr(),
        ..unsafe { zeroed() }
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let preview = std::env::args().any(|argument| argument == "--ui-preview");
    let page = if pairing_preview || visible && state.snapshot().phones.is_empty() && !preview {
        Page::Pairing
    } else if preview {
        match std::env::var("FLOWTYPE_UI_PREVIEW_PAGE").as_deref() {
            Ok("files") => Page::Files,
            Ok("phones") => Page::Phones,
            Ok("settings") => Page::Settings,
            Ok("pairing") => Page::Pairing,
            _ => Page::Status,
        }
    } else {
        Page::Status
    };
    let context = Box::new(UiContext::new(state, host, page));
    let context_ptr = Box::into_raw(context);
    let title = wide(product_name());
    let system_dpi = unsafe { GetDpiForSystem() }.max(96) as i32;
    let initial_width = 980 * system_dpi / 96;
    let initial_height = 620 * system_dpi / 96;
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            initial_width,
            initial_height,
            null_mut(),
            null_mut(),
            instance,
            context_ptr.cast(),
        )
    };
    if hwnd.is_null() {
        unsafe { drop(Box::from_raw(context_ptr)) };
        return Err(std::io::Error::last_os_error().into());
    }
    if visible {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd)
        };
    }
    let mut message: MSG = unsafe { zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        if unsafe { IsDialogMessageW(hwnd, &message) } == 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message)
            };
        }
    }
    Ok(())
}

fn set_control_text(control: HWND, value: &str) {
    if control.is_null() {
        return;
    }
    if window_text(control) == value {
        return;
    }
    let value = wide(value);
    unsafe {
        SetWindowTextW(control, value.as_ptr());
    }
}

fn set_control_visible(control: HWND, visible: bool) {
    if control.is_null() {
        return;
    }
    if (unsafe { IsWindowVisible(control) } != 0) != visible {
        unsafe { ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE }) };
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
        let context = create.lpCreateParams as *mut UiContext;
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, context as isize) };
        unsafe {
            (*context).hwnd = hwnd;
            (*context).state.set_ui_hwnd(hwnd as isize);
        }
    }
    let context = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut UiContext };
    if context.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let ui = unsafe { &mut *context };
    match message {
        WM_CREATE => {
            crate::diagnostics::log("window create");
            ui.rebuild_fonts();
            ui.ensure_painter();
            ui.create_shell();
            ui.rebuild_page();
            unsafe { DragAcceptFiles(hwnd, 1) };
            ui.add_tray();
            if ui.floating_enabled_checked {
                ui.ball_hwnd = ui_floating::create(ui.state.clone(), hwnd);
            }
            0
        }
        WM_NCCALCSIZE | WM_NCPAINT => 0,
        WM_NCACTIVATE => 1,
        WM_NCHITTEST => {
            let result = unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            if result != HTCLIENT as isize {
                return result;
            }
            let mut point = POINT {
                x: lparam as i16 as i32,
                y: (lparam >> 16) as i16 as i32,
            };
            let mut window_rect: RECT = unsafe { zeroed() };
            unsafe { GetWindowRect(hwnd, &mut window_rect) };
            point.x -= window_rect.left;
            point.y -= window_rect.top;
            let (width, height) = ui.client_size();
            let edge = ui.scale(6);
            if point.y >= height - edge {
                return if point.x < edge {
                    HTBOTTOMLEFT
                } else if point.x >= width - edge {
                    HTBOTTOMRIGHT
                } else {
                    HTBOTTOM
                } as isize;
            }
            if point.y < edge {
                return if point.x < edge {
                    HTTOPLEFT
                } else if point.x >= width - edge {
                    HTTOPRIGHT
                } else {
                    HTTOP
                } as isize;
            }
            if point.x < edge {
                return HTLEFT as isize;
            }
            if point.x >= width - edge {
                return HTRIGHT as isize;
            }
            if point.y < ui.scale(52) && point.x < width - ui.scale(100) {
                return HTCAPTION as isize;
            }
            HTCLIENT as isize
        }
        WM_COMMAND => {
            let id = wparam & 0xffff;
            if id == ID_NAME && matches!((wparam >> 16) as u32, EN_SETFOCUS | EN_KILLFOCUS) {
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            match command_for_id(id) {
                Some(UiCommand::Navigate(page)) => {
                    ui.set_page(page);
                    if page == Page::Status {
                        show_window(hwnd);
                    }
                }
                Some(UiCommand::Pair) => {
                    ui.set_page(Page::Pairing);
                    show_window(hwnd);
                }
                Some(UiCommand::SaveSettings) => ui.save_settings(),
                Some(UiCommand::ToggleAutoStart) => {
                    ui.auto_start_checked = !ui.auto_start_checked;
                    unsafe { InvalidateRect(ui.auto_start, null(), 1) };
                }
                Some(UiCommand::ToggleFloating) => {
                    ui.floating_enabled_checked = !ui.floating_enabled_checked;
                    unsafe { InvalidateRect(ui.show_floating, null(), 1) };
                }
                Some(UiCommand::RepairInjector) => ui.repair_injector(),
                Some(UiCommand::UpdateAction) => ui.handle_update_action(),
                Some(UiCommand::OpenUpdateRepository) => {
                    let _ = ui.state.open_update_repository();
                }
                Some(UiCommand::OpenUpdateHistory) => {
                    let _ = ui.state.open_update_history();
                }
                Some(UiCommand::OpenLanguageMenu) => ui.show_language_menu(),
                Some(UiCommand::ChooseFiles) => ui.choose_and_send_files(false),
                Some(UiCommand::ChooseFolder) => ui.choose_and_send_files(true),
                Some(UiCommand::CancelFileTransfer) => ui.cancel_file_transfer(),
                Some(UiCommand::ChangeReceiveFolder) => ui.change_receive_folder(),
                Some(UiCommand::MinimizeWindow) => {
                    unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
                }
                Some(UiCommand::CloseWindow) => {
                    unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0) };
                }
                Some(UiCommand::FileSendTab) => ui.set_file_tab(FileTab::Send),
                Some(UiCommand::FileReceivedTab) => ui.set_file_tab(FileTab::Received),
                Some(UiCommand::Exit) => {
                    crate::diagnostics::log(format!("window command exit id={id}"));
                    unsafe { DestroyWindow(hwnd) };
                }
                Some(UiCommand::Unpair(index)) => ui.confirm_unpair(index),
                None => {}
            }
            0
        }
        WM_APP_STATE => {
            ui.state.begin_ui_update();
            ui.update_from_state();
            0
        }
        update::WM_APP_UPDATE => {
            ui.refresh_update_controls();
            0
        }
        WM_TIMER if wparam == NOTICE_TIMER => {
            unsafe { KillTimer(hwnd, NOTICE_TIMER) };
            ui.save_notice = None;
            unsafe { InvalidateRect(hwnd, null(), 1) };
            0
        }
        WM_APP_SHOW => {
            ui.set_page(Page::Status);
            show_window(hwnd);
            0
        }
        WM_APP_FLOATING_HIDE => {
            ui.floating_enabled_checked = false;
            let _ = settings::set_floating_enabled(false);
            ui.apply_floating_visibility();
            if ui.page == Page::Settings {
                ui.rebuild_page();
            }
            0
        }
        WM_APP_TRAY => {
            match lparam as u32 {
                WM_LBUTTONDBLCLK => {
                    ui.set_page(Page::Status);
                    show_window(hwnd);
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => ui.show_tray_menu(),
                _ => {}
            }
            0
        }
        WM_PAINT => {
            ui.paint();
            0
        }
        WM_ERASEBKGND => 1,
        WM_DROPFILES => {
            ui.send_dropped_paths(wparam as _);
            0
        }
        WM_SIZE => {
            if let Some(painter) = ui.direct2d.as_ref() {
                let (width, height) = ui.client_size();
                if let Err(error) = painter.resize(width as u32, height as u32) {
                    crate::diagnostics::log(format!("UI target resize: {error}"));
                    ui.direct2d = None;
                }
            }
            ui.ensure_painter();
            if wparam != SIZE_MINIMIZED as usize {
                ui.reposition_responsive_controls();
            }
            0
        }
        WM_EXITSIZEMOVE => {
            ui.reposition_responsive_controls();
            0
        }
        WM_DRAWITEM => {
            let item = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
            ui.draw_button(item);
            1
        }
        WM_CTLCOLORSTATIC => {
            let control = lparam as HWND;
            unsafe {
                SetBkMode(wparam as _, TRANSPARENT as i32);
                SetTextColor(
                    wparam as _,
                    if ui.muted_controls.contains(&control) {
                        COLOR_MUTED
                    } else {
                        COLOR_TEXT
                    },
                );
                GetStockObject(WHITE_BRUSH) as LRESULT
            }
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam as *const RECT) };
            unsafe {
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            if let Some(painter) = ui.direct2d.as_ref() {
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
                painter.set_dpi(dpi);
            }
            ui.ensure_painter();
            ui.rebuild_fonts();
            ui.refresh_shell();
            ui.rebuild_page();
            0
        }
        WM_DISPLAYCHANGE => {
            ui.direct2d = None;
            ui.ensure_painter();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            info.ptMinTrackSize.x = ui.scale(MIN_CLIENT_WIDTH);
            info.ptMinTrackSize.y = ui.scale(MIN_CLIENT_HEIGHT);
            0
        }
        WM_CLOSE => {
            crate::diagnostics::log("window close -> hide");
            if ui.page == Page::Pairing {
                ui.state.cancel_pairing();
            }
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        WM_DESTROY => {
            crate::diagnostics::log("window destroy");
            if !ui.ball_hwnd.is_null() {
                unsafe { DestroyWindow(ui.ball_hwnd) };
                ui.ball_hwnd = null_mut();
            }
            ui.remove_tray();
            ui.state.set_ui_hwnd(0);
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_NCDESTROY => {
            crate::diagnostics::log("window nc_destroy");
            for font in [
                ui.title_font,
                ui.heading_font,
                ui.navigation_font,
                ui.body_font,
                ui.icon_font,
            ] {
                if !font.is_null() {
                    unsafe { DeleteObject(font) };
                }
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

fn create_navigation(ui: &mut UiContext) {
    ui.owner_button(
        tr("状态", "Status"),
        ID_NAV_STATUS,
        0,
        120,
        202,
        42,
        ButtonKind::Navigation(Page::Status),
    );
    ui.owner_button(
        tr("文件传输", "File transfer"),
        ID_NAV_FILES,
        0,
        166,
        202,
        42,
        ButtonKind::Navigation(Page::Files),
    );
    ui.owner_button(
        tr("已绑定手机", "Paired phones"),
        ID_NAV_PHONES,
        0,
        212,
        202,
        42,
        ButtonKind::Navigation(Page::Phones),
    );
    ui.owner_button(
        tr("设置", "Settings"),
        ID_NAV_SETTINGS,
        0,
        258,
        202,
        42,
        ButtonKind::Navigation(Page::Settings),
    );
}

fn show_window(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd)
    };
}

pub fn show_existing_window() {
    let class_name = wide(CLASS_NAME);
    for _ in 0..20 {
        let hwnd = unsafe { FindWindowW(class_name.as_ptr(), null()) };
        if !hwnd.is_null() {
            unsafe { PostMessageW(hwnd, WM_APP_SHOW, 0, 0) };
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
