use std::mem::{size_of, zeroed};
use std::net::IpAddr;
use std::ptr::{null, null_mut};
use std::sync::Arc;

use qrcode::{Color, QrCode};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_WINDOW, CreateSolidBrush, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
    DeleteObject, DrawFocusRect, EndPaint, FW_NORMAL, FW_SEMIBOLD, FillRect, GetStockObject,
    HBRUSH, HFONT, InvalidateRect, PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE,
    RDW_UPDATENOW, RedrawWindow, SetBkMode, SetTextColor, TRANSPARENT, UpdateWindow, WHITE_BRUSH,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, ODS_FOCUS, ODS_SELECTED, PBM_SETPOS, PBM_SETRANGE32, PBS_SMOOTH,
};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::{AppState, PORT, WM_APP_STATE, settings, update};

#[path = "ui_commands.rs"]
mod ui_commands;
#[path = "ui_floating.rs"]
mod ui_floating;
#[path = "ui_paint.rs"]
mod ui_paint;
#[path = "ui_theme.rs"]
mod ui_theme;

use ui_commands::*;
use ui_paint::*;
use ui_theme::*;

const CLASS_NAME: &str = "FlowTypeMainWindow";
const WM_APP_TRAY: u32 = 0x8002;
const WM_APP_SHOW: u32 = 0x8003;
const WM_APP_FLOATING_HIDE: u32 = 0x8004;
const TRAY_ID: u32 = 1;
const NOTICE_TIMER: usize = 2;

struct SaveNotice {
    success: bool,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Status,
    Phones,
    Settings,
    Pairing,
}

#[derive(Clone, Copy)]
enum ButtonKind {
    Navigation(Page),
    Secondary,
    Text,
    Back,
    Checkbox,
}

struct UiContext {
    state: Arc<AppState>,
    host: IpAddr,
    hwnd: HWND,
    page: Page,
    controls: Vec<HWND>,
    buttons: Vec<(HWND, ButtonKind)>,
    muted_controls: Vec<HWND>,
    phone_ids: Vec<String>,
    qr: Option<(usize, Vec<bool>)>,
    name_edit: HWND,
    auto_start: HWND,
    show_floating: HWND,
    update_status: HWND,
    update_progress: HWND,
    update_action: HWND,
    update_repository: HWND,
    update_history: HWND,
    title_font: HFONT,
    heading_font: HFONT,
    body_font: HFONT,
    icon_font: HFONT,
    auto_start_checked: bool,
    floating_enabled_checked: bool,
    ball_hwnd: HWND,
    save_notice: Option<SaveNotice>,
}

impl UiContext {
    fn new(state: Arc<AppState>, host: IpAddr, page: Page) -> Self {
        Self {
            state,
            host,
            hwnd: null_mut(),
            page,
            controls: Vec::new(),
            buttons: Vec::new(),
            muted_controls: Vec::new(),
            phone_ids: Vec::new(),
            qr: None,
            name_edit: null_mut(),
            auto_start: null_mut(),
            show_floating: null_mut(),
            update_status: null_mut(),
            update_progress: null_mut(),
            update_action: null_mut(),
            update_repository: null_mut(),
            update_history: null_mut(),
            title_font: null_mut(),
            heading_font: null_mut(),
            body_font: null_mut(),
            icon_font: null_mut(),
            auto_start_checked: false,
            floating_enabled_checked: settings::floating_enabled(),
            ball_hwnd: null_mut(),
            save_notice: None,
        }
    }

    fn scale(&self, value: i32) -> i32 {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        value * dpi as i32 / 96
    }

    fn rebuild_fonts(&mut self) {
        for font in [
            self.title_font,
            self.heading_font,
            self.body_font,
            self.icon_font,
        ] {
            if !font.is_null() {
                unsafe { DeleteObject(font) };
            }
        }
        self.title_font = create_font(self.scale(22), FW_SEMIBOLD as i32, "Segoe UI");
        self.heading_font = create_font(self.scale(17), FW_SEMIBOLD as i32, "Segoe UI");
        self.body_font = create_font(self.scale(14), FW_NORMAL as i32, "Segoe UI");
        self.icon_font = create_font(self.scale(20), FW_NORMAL as i32, "Segoe Fluent Icons");
    }

    fn rebuild_page(&mut self) {
        unsafe { SendMessageW(self.hwnd, WM_SETREDRAW, 0, 0) };
        for control in self.controls.drain(..) {
            unsafe { DestroyWindow(control) };
        }
        self.phone_ids.clear();
        self.buttons.clear();
        self.muted_controls.clear();
        self.qr = None;
        self.name_edit = null_mut();
        self.auto_start = null_mut();
        self.show_floating = null_mut();
        self.update_status = null_mut();
        self.update_progress = null_mut();
        self.update_action = null_mut();
        self.update_repository = null_mut();
        self.update_history = null_mut();
        create_navigation(self);
        match self.page {
            Page::Status => self.build_status(),
            Page::Phones => self.build_phones(),
            Page::Settings => self.build_settings(),
            Page::Pairing => self.build_pairing(),
        }
        self.update_tray();
        unsafe {
            SendMessageW(self.hwnd, WM_SETREDRAW, 1, 0);
            RedrawWindow(
                self.hwnd,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW | RDW_ALLCHILDREN,
            );
        }
    }

    fn build_status(&mut self) {
        let snapshot = self.state.snapshot();
        self.text(&snapshot.status.summary, 258, 43, 430, 34, self.title_font);
        self.label_value(
            "手机",
            snapshot
                .status
                .connected_phone
                .as_deref()
                .unwrap_or("尚未连接"),
            105,
        );
        self.label_value("输入状态", "等待手机输入", 151);
        self.label_value(
            "输入位置",
            snapshot.status.target_name.as_deref().unwrap_or("尚未选择"),
            205,
        );
        self.label_value("连接地址", &format!("{}:{PORT}", self.host), 259);
        if let Some(error) = snapshot.status.last_error.as_deref() {
            self.muted_text(error, 220, 302, 470, 42, self.body_font);
        }
        self.owner_button(
            "绑定手机",
            ID_PAIR,
            220,
            348,
            142,
            42,
            ButtonKind::Secondary,
        );
    }

    fn build_phones(&mut self) {
        let snapshot = self.state.snapshot();
        self.title("已绑定手机");
        self.owner_button("绑定手机", ID_PAIR, 590, 28, 132, 42, ButtonKind::Secondary);
        if snapshot.phones.is_empty() {
            self.muted_text("还没有绑定手机", 220, 112, 420, 30, self.body_font);
        }
        for (index, (phone_id, phone)) in snapshot.phones.iter().enumerate() {
            let y = 102 + index as i32 * 84;
            self.text(&phone.phone_name, 268, y, 260, 28, self.heading_font);
            let connected = snapshot.status.connected_phone.as_deref() == Some(&phone.phone_name);
            let state_text = if connected { "已连接" } else { "未连接" };
            self.text(state_text, 288, y + 34, 90, 24, self.body_font);
            let detail = if connected {
                "刚刚连接"
            } else if phone.last_connected.is_some() {
                "最近连接过"
            } else {
                "尚未连接过"
            };
            self.muted_text(detail, 380, y + 34, 190, 24, self.body_font);
            self.owner_button(
                "解除绑定",
                ID_UNPAIR_BASE + index,
                620,
                y + 13,
                94,
                34,
                ButtonKind::Text,
            );
            self.phone_ids.push(phone_id.clone());
        }
    }

    fn build_settings(&mut self) {
        let snapshot = self.state.snapshot();
        self.title("设置");
        self.text("常规", 220, 92, 120, 26, self.heading_font);
        self.text("电脑名称", 220, 134, 110, 28, self.body_font);
        self.name_edit = self.control(
            "EDIT",
            &snapshot.pc_name,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
            ID_NAME,
            350,
            132,
            270,
            27,
            self.body_font,
        );
        self.auto_start_checked = settings::auto_start_enabled();
        self.auto_start = self.owner_button(
            "开机启动",
            ID_AUTO_START,
            220,
            174,
            330,
            34,
            ButtonKind::Checkbox,
        );
        self.floating_enabled_checked = settings::floating_enabled();
        self.show_floating = self.owner_button(
            "显示悬浮球",
            ID_SHOW_FLOATING,
            220,
            214,
            220,
            34,
            ButtonKind::Checkbox,
        );
        self.owner_button(
            "保存设置",
            ID_SAVE_SETTINGS,
            610,
            34,
            112,
            34,
            ButtonKind::Secondary,
        );

        self.text("输入服务", 220, 272, 120, 26, self.heading_font);
        let service = if snapshot.injector_ready {
            "正常运行"
        } else {
            "输入服务不可用"
        };
        self.text(service, 246, 308, 180, 28, self.body_font);
        self.owner_button(
            "修复输入服务",
            ID_REPAIR,
            610,
            298,
            112,
            38,
            ButtonKind::Secondary,
        );
        self.text("版本与更新", 220, 370, 160, 26, self.heading_font);
        self.text(
            &format!("当前版本 {} · Windows x64", env!("CARGO_PKG_VERSION")),
            220,
            406,
            350,
            24,
            self.body_font,
        );
        self.update_status = self.muted_text("", 220, 434, 360, 24, self.body_font);
        self.update_progress = self.control(
            "msctls_progress32",
            "",
            WS_CHILD | PBS_SMOOTH,
            0,
            220,
            462,
            360,
            6,
            self.body_font,
        );
        self.update_repository = self.owner_button(
            "GitHub 仓库",
            ID_UPDATE_REPOSITORY,
            370,
            476,
            112,
            32,
            ButtonKind::Secondary,
        );
        self.update_history = self.owner_button(
            "查看更新",
            ID_UPDATE_HISTORY,
            490,
            476,
            112,
            32,
            ButtonKind::Secondary,
        );
        self.update_action = self.owner_button(
            "检查更新",
            ID_UPDATE_ACTION,
            610,
            474,
            112,
            34,
            ButtonKind::Secondary,
        );
        self.muted_text("检查更新会连接 GitHub。", 220, 518, 500, 34, self.body_font);
        self.refresh_update_controls();
    }

    fn build_pairing(&mut self) {
        self.owner_button("", ID_NAV_PHONES, 202, 32, 34, 34, ButtonKind::Back);
        self.text("绑定手机", 246, 34, 300, 34, self.heading_font);
        self.text("在手机上打开说写，", 470, 145, 250, 30, self.heading_font);
        self.text("扫描此二维码", 470, 178, 220, 30, self.heading_font);
        let pc_name = self.state.snapshot().pc_name;
        self.text(
            &format!("此电脑：{pc_name}"),
            470,
            252,
            250,
            28,
            self.body_font,
        );
        self.text("等待手机扫描", 486, 294, 180, 26, self.body_font);
        self.muted_text(
            "二维码仅用于本次绑定，绑定成功后立即失效",
            470,
            334,
            270,
            48,
            self.body_font,
        );
        match self.state.begin_pairing(self.host) {
            Ok(uri) => match QrCode::new(uri.as_bytes()) {
                Ok(code) => {
                    self.qr = Some((
                        code.width(),
                        code.to_colors()
                            .into_iter()
                            .map(|color| color == Color::Dark)
                            .collect(),
                    ));
                }
                Err(_) => {
                    self.text("二维码生成失败", 220, 140, 220, 30, self.body_font);
                }
            },
            Err(_) => {
                self.text(
                    "无法开始绑定，请重新打开此页面",
                    220,
                    140,
                    430,
                    30,
                    self.body_font,
                );
            }
        }
    }

    fn title(&mut self, value: &str) {
        self.text(value, 220, 34, 360, 40, self.title_font);
    }

    fn label_value(&mut self, label: &str, value: &str, y: i32) {
        self.muted_text(label, 220, y, 120, 28, self.body_font);
        self.text(value, 370, y, 320, 28, self.body_font);
    }

    fn text(&mut self, value: &str, x: i32, y: i32, width: i32, height: i32, font: HFONT) -> HWND {
        self.control(
            "STATIC",
            value,
            WS_CHILD | WS_VISIBLE,
            0,
            x,
            y,
            width,
            height,
            font,
        )
    }

    fn muted_text(
        &mut self,
        value: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        font: HFONT,
    ) -> HWND {
        let control = self.text(value, x, y, width, height, font);
        self.muted_controls.push(control);
        control
    }

    #[allow(clippy::too_many_arguments)]
    fn owner_button(
        &mut self,
        value: &str,
        id: usize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        kind: ButtonKind,
    ) -> HWND {
        let control = self.control(
            "BUTTON",
            value,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
            id,
            x,
            y,
            width,
            height,
            self.body_font,
        );
        self.buttons.push((control, kind));
        control
    }

    #[allow(clippy::too_many_arguments)]
    fn control(
        &mut self,
        class: &str,
        text: &str,
        style: u32,
        id: usize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        font: HFONT,
    ) -> HWND {
        let class = wide(class);
        let text = wide(text);
        let control = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                style,
                self.scale(x),
                self.scale(y),
                self.scale(width),
                self.scale(height),
                self.hwnd,
                id as _,
                GetModuleHandleW(null()),
                null(),
            )
        };
        if !control.is_null() {
            unsafe { SendMessageW(control, WM_SETFONT, font as usize, 1) };
            self.controls.push(control);
        }
        control
    }

    fn set_page(&mut self, page: Page) {
        if self.page == Page::Pairing && page != Page::Pairing {
            self.state.cancel_pairing();
        }
        self.page = page;
        self.rebuild_page();
    }

    fn save_settings(&mut self) {
        let name = window_text(self.name_edit);
        let result = self
            .state
            .rename_computer(&name)
            .and_then(|_| settings::set_auto_start(self.auto_start_checked).map_err(Into::into))
            .and_then(|_| {
                settings::set_floating_enabled(self.floating_enabled_checked).map_err(Into::into)
            });
        if result.is_ok() {
            self.apply_floating_visibility();
        }
        let (success, text) = if result.is_ok() {
            (true, "设置已保存")
        } else {
            (false, "设置保存失败，请检查电脑名称")
        };
        self.rebuild_page();
        self.show_save_notice(success, text);
    }

    fn show_save_notice(&mut self, success: bool, text: &str) {
        self.save_notice = Some(SaveNotice {
            success,
            text: text.to_owned(),
        });
        unsafe { SetTimer(self.hwnd, NOTICE_TIMER, 2600, None) };
        unsafe { InvalidateRect(self.hwnd, null(), 1) };
    }

    fn apply_floating_visibility(&mut self) {
        if self.floating_enabled_checked {
            if self.ball_hwnd.is_null() {
                self.ball_hwnd = ui_floating::create(self.state.clone(), self.hwnd);
            }
        } else if !self.ball_hwnd.is_null() {
            unsafe { DestroyWindow(self.ball_hwnd) };
            self.ball_hwnd = null_mut();
        }
    }

    fn repair_injector(&mut self) {
        let result = self.state.repair_injector();
        message_box(
            self.hwnd,
            if result.is_ok() {
                "输入服务已恢复"
            } else {
                "无法启动输入服务，请重新运行安装程序"
            },
            "说写",
            if result.is_ok() {
                MB_OK | MB_ICONINFORMATION
            } else {
                MB_OK | MB_ICONERROR
            },
        );
        self.rebuild_page();
    }

    fn confirm_unpair(&mut self, index: usize) {
        let Some(phone_id) = self.phone_ids.get(index).cloned() else {
            return;
        };
        let snapshot = self.state.snapshot();
        let name = snapshot
            .phones
            .iter()
            .find(|(id, _)| id == &phone_id)
            .map(|(_, phone)| phone.phone_name.as_str())
            .unwrap_or("这部手机");
        if message_box(
            self.hwnd,
            &format!("解绑“{name}”？解绑后需要重新扫描二维码。"),
            "解绑手机",
            MB_YESNO | MB_ICONWARNING,
        ) == IDYES
        {
            let _ = self.state.unpair_phone(&phone_id);
            self.rebuild_page();
        }
    }

    fn update_from_state(&mut self) {
        if self.page == Page::Pairing && self.state.current_pairing_uri(self.host).is_none() {
            self.page = Page::Status;
        }
        self.rebuild_page();
        ui_floating::refresh(self.ball_hwnd);
    }

    fn refresh_update_controls(&mut self) {
        if self.page != Page::Settings || self.update_status.is_null() {
            self.update_tray();
            return;
        }
        let snapshot = self.state.snapshot().update;
        let status = if snapshot.action == update::UpdateAction::Install
            && self.state.update_install_blocked()
        {
            "输入结束后可安装".to_owned()
        } else {
            snapshot.message.clone()
        };
        unsafe {
            SetWindowTextW(self.update_status, wide(&status).as_ptr());
            SetWindowTextW(self.update_action, wide(&snapshot.action_label).as_ptr());
            ShowWindow(
                self.update_action,
                if snapshot.action == update::UpdateAction::None {
                    SW_HIDE
                } else {
                    SW_SHOW
                },
            );
            if let Some((transferred, total)) = snapshot.progress {
                let position = if total == 0 {
                    0
                } else {
                    transferred
                        .saturating_mul(1000)
                        .saturating_div(total)
                        .min(1000) as isize
                };
                SendMessageW(self.update_progress, PBM_SETRANGE32, 0, 1000);
                SendMessageW(self.update_progress, PBM_SETPOS, position as usize, 0);
                ShowWindow(self.update_progress, SW_SHOW);
            } else {
                ShowWindow(self.update_progress, SW_HIDE);
            }
            InvalidateRect(self.update_action, null(), 1);
            InvalidateRect(self.update_repository, null(), 1);
            InvalidateRect(self.update_history, null(), 1);
        }
        self.update_tray();
    }

    fn handle_update_action(&mut self) {
        let action = self.state.snapshot().update.action;
        if action == update::UpdateAction::Install {
            if self.state.update_install_blocked() {
                self.show_save_notice(false, "请先完成当前输入，再安装更新");
                return;
            }
            if self.state.install_update().is_ok() {
                unsafe { DestroyWindow(self.hwnd) };
            }
        } else {
            self.state.perform_update_action(action);
        }
    }

    fn update_tray(&self) {
        let mut data = self.tray_data();
        data.uFlags = NIF_TIP;
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = TRAY_ID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_APP_TRAY;
        data.hIcon = app_icon();
        let status = self.state.snapshot().status.summary;
        copy_wide(&mut data.szTip, &format!("说写 · {status}"));
        data
    }

    fn add_tray(&self) {
        let data = self.tray_data();
        unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    }

    fn remove_tray(&self) {
        let data = self.tray_data();
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    }

    fn show_tray_menu(&mut self) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }
        let status = wide(&self.state.snapshot().status.summary);
        unsafe {
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, status.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            append_menu(menu, ID_TRAY_OPEN, "打开说写");
            append_menu(menu, ID_TRAY_PAIR, "绑定手机...");
            if let Some(label) = self.state.snapshot().update.tray_label() {
                append_menu(menu, ID_TRAY_UPDATE, &label);
            }
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            append_menu(menu, ID_TRAY_EXIT, "退出说写");
            let mut point: POINT = zeroed();
            GetCursorPos(&mut point);
            SetForegroundWindow(self.hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                0,
                self.hwnd,
                null(),
            );
            DestroyMenu(menu);
            if command != 0 {
                SendMessageW(self.hwnd, WM_COMMAND, command as usize, 0);
            }
        }
    }

    fn draw_button(&self, item: &DRAWITEMSTRUCT) {
        let Some((_, kind)) = self
            .buttons
            .iter()
            .find(|(window, _)| *window == item.hwndItem)
        else {
            return;
        };
        let pressed = item.itemState & ODS_SELECTED != 0;
        let focused = item.itemState & ODS_FOCUS != 0;
        let mut rect = item.rcItem;
        let text = window_text(item.hwndItem);
        unsafe { SetBkMode(item.hDC, TRANSPARENT as i32) };

        match kind {
            ButtonKind::Navigation(target) => {
                let selected = self.selected_navigation() == *target;
                fill(
                    item.hDC,
                    &rect,
                    if selected {
                        COLOR_TEAL_PALE
                    } else {
                        COLOR_SIDEBAR
                    },
                );
                if selected {
                    let marker = RECT {
                        right: self.scale(4),
                        ..rect
                    };
                    fill(item.hDC, &marker, COLOR_TEAL);
                }
                let icon = match target {
                    Page::Status => "\u{e7f4}",
                    Page::Phones | Page::Pairing => "\u{e8ea}",
                    Page::Settings => "\u{e713}",
                };
                let icon_rect = RECT {
                    left: self.scale(22),
                    right: self.scale(58),
                    ..rect
                };
                draw_label(
                    item.hDC,
                    icon,
                    icon_rect,
                    self.icon_font,
                    if selected { COLOR_TEAL } else { COLOR_TEXT },
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                rect.left = self.scale(62);
                draw_label(
                    item.hDC,
                    &text,
                    rect,
                    self.heading_font,
                    COLOR_TEXT,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Secondary => {
                fill(
                    item.hDC,
                    &rect,
                    if pressed { 0x00f3_f3f3 } else { COLOR_WHITE },
                );
                outline_round_rect(item.hDC, rect, COLOR_LINE, self.scale(4));
                let mut text_rect = rect;
                if item.CtlID as usize == ID_PAIR {
                    let icon_rect = RECT {
                        left: rect.left + self.scale(10),
                        right: rect.left + self.scale(38),
                        ..rect
                    };
                    draw_label(
                        item.hDC,
                        "\u{e8ea}",
                        icon_rect,
                        self.icon_font,
                        COLOR_TEAL,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                    text_rect.left += self.scale(26);
                }
                draw_label(
                    item.hDC,
                    &text,
                    text_rect,
                    self.body_font,
                    COLOR_TEXT,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Text => {
                fill(item.hDC, &rect, COLOR_WHITE);
                draw_label(
                    item.hDC,
                    &text,
                    rect,
                    self.body_font,
                    COLOR_TEXT,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Back => {
                fill(
                    item.hDC,
                    &rect,
                    if pressed { 0x00f3_f3f3 } else { COLOR_WHITE },
                );
                draw_label(
                    item.hDC,
                    "\u{e72b}",
                    rect,
                    self.icon_font,
                    COLOR_TEXT,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Checkbox => {
                let checked = if item.CtlID as usize == ID_SHOW_FLOATING {
                    self.floating_enabled_checked
                } else {
                    self.auto_start_checked
                };
                fill(item.hDC, &rect, COLOR_WHITE);
                let box_size = self.scale(18);
                let top = (rect.top + rect.bottom - box_size) / 2;
                let box_rect = RECT {
                    left: rect.left,
                    top,
                    right: rect.left + box_size,
                    bottom: top + box_size,
                };
                fill(
                    item.hDC,
                    &box_rect,
                    if checked { COLOR_TEAL } else { COLOR_WHITE },
                );
                outline_round_rect(
                    item.hDC,
                    box_rect,
                    if checked { COLOR_TEAL } else { COLOR_LINE },
                    self.scale(2),
                );
                if checked {
                    draw_label(
                        item.hDC,
                        "\u{e73e}",
                        box_rect,
                        self.icon_font,
                        COLOR_WHITE,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                }
                rect.left += box_size + self.scale(10);
                draw_label(
                    item.hDC,
                    &text,
                    rect,
                    self.body_font,
                    COLOR_TEXT,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
            }
        }
        if focused && !matches!(kind, ButtonKind::Navigation(_)) {
            let focus = if matches!(kind, ButtonKind::Checkbox) {
                let text_width = measure_text_width(item.hDC, &text, self.body_font);
                let text_height = self.scale(20);
                RECT {
                    left: rect.left - self.scale(2),
                    top: (rect.top + rect.bottom - text_height) / 2,
                    right: rect.left + text_width + self.scale(2),
                    bottom: (rect.top + rect.bottom + text_height) / 2,
                }
            } else {
                RECT {
                    left: item.rcItem.left + self.scale(3),
                    top: item.rcItem.top + self.scale(3),
                    right: item.rcItem.right - self.scale(3),
                    bottom: item.rcItem.bottom - self.scale(3),
                }
            };
            unsafe { DrawFocusRect(item.hDC, &focus) };
        }
    }

    fn selected_navigation(&self) -> Page {
        if self.page == Page::Pairing {
            Page::Phones
        } else {
            self.page
        }
    }

    fn paint(&self) {
        let mut paint: PAINTSTRUCT = unsafe { zeroed() };
        let dc = unsafe { BeginPaint(self.hwnd, &mut paint) };
        let mut client: RECT = unsafe { zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut client) };
        fill(dc, &client, COLOR_WHITE);
        let sidebar = RECT {
            right: self.scale(180),
            ..client
        };
        fill(dc, &sidebar, COLOR_SIDEBAR);
        draw_line(
            dc,
            self.scale(180),
            0,
            self.scale(180),
            client.bottom,
            COLOR_LINE,
        );

        match self.page {
            Page::Status => {
                let circle = RECT {
                    left: self.scale(220),
                    top: self.scale(44),
                    right: self.scale(248),
                    bottom: self.scale(72),
                };
                fill_ellipse(dc, circle, COLOR_TEAL);
                draw_label(
                    dc,
                    "\u{e73e}",
                    circle,
                    self.icon_font,
                    COLOR_WHITE,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                for y in [140, 194, 248, 292] {
                    draw_line(
                        dc,
                        self.scale(220),
                        self.scale(y),
                        self.scale(708),
                        self.scale(y),
                        COLOR_LINE,
                    );
                }
            }
            Page::Phones => {
                let snapshot = self.state.snapshot();
                for index in 0..self.phone_ids.len() {
                    let y = 174 + index as i32 * 84;
                    draw_line(
                        dc,
                        self.scale(220),
                        self.scale(y),
                        self.scale(716),
                        self.scale(y),
                        COLOR_LINE,
                    );
                    let icon_rect = RECT {
                        left: self.scale(222),
                        top: self.scale(y - 69),
                        right: self.scale(256),
                        bottom: self.scale(y - 29),
                    };
                    draw_label(
                        dc,
                        "\u{e8ea}",
                        icon_rect,
                        self.icon_font,
                        COLOR_TEXT,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                    let dot = RECT {
                        left: self.scale(268),
                        top: self.scale(y - 40),
                        right: self.scale(278),
                        bottom: self.scale(y - 30),
                    };
                    let connected = snapshot.phones.get(index).is_some_and(|(_, phone)| {
                        snapshot.status.connected_phone.as_deref()
                            == Some(phone.phone_name.as_str())
                    });
                    fill_ellipse(dc, dot, if connected { COLOR_TEAL } else { 0x00cc_cccc });
                }
            }
            Page::Settings => {
                draw_line(
                    dc,
                    self.scale(204),
                    self.scale(262),
                    self.scale(722),
                    self.scale(262),
                    COLOR_LINE,
                );
                draw_line(
                    dc,
                    self.scale(204),
                    self.scale(382),
                    self.scale(722),
                    self.scale(382),
                    COLOR_LINE,
                );
                let dot = RECT {
                    left: self.scale(220),
                    top: self.scale(339),
                    right: self.scale(232),
                    bottom: self.scale(351),
                };
                fill_ellipse(
                    dc,
                    dot,
                    if self.state.snapshot().injector_ready {
                        COLOR_TEAL
                    } else {
                        COLOR_DANGER
                    },
                );
            }
            Page::Pairing => {
                let dot = RECT {
                    left: self.scale(470),
                    top: self.scale(301),
                    right: self.scale(478),
                    bottom: self.scale(309),
                };
                fill_ellipse(dc, dot, COLOR_TEAL);
            }
        }
        if let Some((width, modules)) = self.qr.as_ref() {
            let module = (self.scale(220) / (*width as i32 + 8)).max(2);
            let quiet = 4;
            let origin_x = self.scale(220);
            let origin_y = self.scale(130);
            let total = (*width as i32 + quiet * 2) * module;
            let white = unsafe { CreateSolidBrush(COLOR_WHITE) };
            let black = unsafe { CreateSolidBrush(0x0000_0000) };
            let border = RECT {
                left: origin_x,
                top: origin_y,
                right: origin_x + total,
                bottom: origin_y + total,
            };
            unsafe { FillRect(dc, &border, white) };
            for row in 0..*width {
                for column in 0..*width {
                    if modules[row * *width + column] {
                        let left = origin_x + (column as i32 + quiet) * module;
                        let top = origin_y + (row as i32 + quiet) * module;
                        let rectangle = RECT {
                            left,
                            top,
                            right: left + module,
                            bottom: top + module,
                        };
                        unsafe { FillRect(dc, &rectangle, black) };
                    }
                }
            }
            unsafe {
                DeleteObject(white);
                DeleteObject(black)
            };
        }
        if let Some(notice) = self.save_notice.as_ref() {
            let toast = RECT {
                left: self.scale(500),
                top: self.scale(430),
                right: self.scale(722),
                bottom: self.scale(478),
            };
            let accent = if notice.success {
                COLOR_TEAL
            } else {
                COLOR_DANGER
            };
            fill_round_rect(
                dc,
                toast,
                if notice.success {
                    COLOR_TEAL_PALE
                } else {
                    0x00f1_e4e4
                },
                self.scale(8),
            );
            let icon = RECT {
                left: toast.left + self.scale(10),
                top: toast.top + self.scale(10),
                right: toast.left + self.scale(34),
                bottom: toast.top + self.scale(34),
            };
            fill_ellipse(dc, icon, accent);
            draw_label(
                dc,
                if notice.success { "\u{e73e}" } else { "!" },
                icon,
                self.icon_font,
                COLOR_WHITE,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_label(
                dc,
                &notice.text,
                RECT {
                    left: toast.left + self.scale(46),
                    top: toast.top,
                    right: toast.right - self.scale(10),
                    bottom: toast.bottom,
                },
                self.body_font,
                COLOR_TEXT,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
        }
        unsafe { EndPaint(self.hwnd, &paint) };
    }
}

pub fn run(
    state: Arc<AppState>,
    host: IpAddr,
    visible: bool,
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
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { zeroed() }
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let page = if visible && state.snapshot().phones.is_empty() {
        Page::Pairing
    } else {
        Page::Status
    };
    let context = Box::new(UiContext::new(state, host, page));
    let context_ptr = Box::into_raw(context);
    let title = wide("说写");
    let system_dpi = unsafe { GetDpiForSystem() }.max(96) as i32;
    let initial_width = 760 * system_dpi / 96;
    let initial_height = 600 * system_dpi / 96;
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
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
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message)
        };
    }
    Ok(())
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
            ui.rebuild_fonts();
            ui.rebuild_page();
            ui.add_tray();
            if ui.floating_enabled_checked {
                ui.ball_hwnd = ui_floating::create(ui.state.clone(), hwnd);
            }
            0
        }
        WM_COMMAND => {
            let id = wparam & 0xffff;
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
                Some(UiCommand::Exit) => {
                    unsafe { DestroyWindow(hwnd) };
                }
                Some(UiCommand::Unpair(index)) => ui.confirm_unpair(index),
                None => {}
            }
            0
        }
        WM_APP_STATE => {
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
            ui.rebuild_fonts();
            ui.rebuild_page();
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            info.ptMinTrackSize.x = ui.scale(680);
            info.ptMinTrackSize.y = ui.scale(560);
            0
        }
        WM_CLOSE => {
            if ui.page == Page::Pairing {
                ui.state.cancel_pairing();
            }
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        WM_DESTROY => {
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
            for font in [ui.title_font, ui.heading_font, ui.body_font, ui.icon_font] {
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
        "状态",
        ID_NAV_STATUS,
        0,
        18,
        180,
        48,
        ButtonKind::Navigation(Page::Status),
    );
    ui.owner_button(
        "已绑定手机",
        ID_NAV_PHONES,
        0,
        66,
        180,
        48,
        ButtonKind::Navigation(Page::Phones),
    );
    ui.owner_button(
        "设置",
        ID_NAV_SETTINGS,
        0,
        114,
        180,
        48,
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
