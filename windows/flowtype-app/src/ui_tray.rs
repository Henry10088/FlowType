use super::*;

impl UiContext {
    pub(super) fn update_tray(&self) {
        let mut data = self.tray_data();
        data.uFlags = NIF_TIP;
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    pub(super) fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = TRAY_ID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_APP_TRAY;
        data.hIcon = app_icon();
        let status = self.state.snapshot().status.summary;
        copy_wide(&mut data.szTip, &format!("{} · {status}", product_name()));
        data
    }

    pub(super) fn add_tray(&self) {
        let data = self.tray_data();
        unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    }

    pub(super) fn remove_tray(&self) {
        let data = self.tray_data();
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    }

    pub(super) fn show_tray_menu(&mut self) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }
        let status = wide(&self.state.snapshot().status.summary);
        unsafe {
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, status.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            append_menu(menu, ID_TRAY_OPEN, tr("打开说写", "Open FlowType"));
            append_menu(menu, ID_TRAY_PAIR, tr("绑定手机...", "Pair phone..."));
            if let Some(label) = self.state.snapshot().update.tray_label() {
                append_menu(menu, ID_TRAY_UPDATE, &label);
            }
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            append_menu(menu, ID_TRAY_EXIT, tr("退出说写", "Exit FlowType"));
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
}
