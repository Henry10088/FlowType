use super::*;

impl UiContext {
    pub(super) fn create_shell(&mut self) {
        create_navigation(self);
        self.create_language_button();
        self.shell_controls = std::mem::take(&mut self.controls);
    }

    pub(super) fn refresh_shell(&self) {
        for control in &self.shell_controls {
            let (top, font, label) = match unsafe { GetDlgCtrlID(*control) } as usize {
                ID_NAV_STATUS => (Some(18), self.navigation_font, Some(tr("状态", "Status"))),
                ID_NAV_FILES => (
                    Some(66),
                    self.navigation_font,
                    Some(tr("文件传输", "File transfer")),
                ),
                ID_NAV_PHONES => (
                    Some(114),
                    self.navigation_font,
                    Some(tr("已绑定手机", "Paired phones")),
                ),
                ID_NAV_SETTINGS => (
                    Some(162),
                    self.navigation_font,
                    Some(tr("设置", "Settings")),
                ),
                ID_LANGUAGE_MENU => (None, self.body_font, None),
                _ => continue,
            };
            unsafe { SendMessageW(*control, WM_SETFONT, font as usize, 1) };
            if let Some(label) = label {
                set_control_text(*control, label);
            }
            if let Some(top) = top {
                unsafe {
                    SetWindowPos(
                        *control,
                        null_mut(),
                        0,
                        self.scale(top),
                        self.scale(180),
                        self.scale(48),
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }
    }

    pub(super) fn logical_client_width(&self) -> f32 {
        let (width, _) = self.client_size();
        width as f32 * 96.0 / unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32
    }

    pub(super) fn phones_layout(&self) -> PhonesLayout {
        PhonesLayout::new(self.logical_client_width(), self.phone_ids.len())
    }

    pub(super) fn reposition_responsive_controls(&self) {
        let phone_layout = self.phones_layout();
        let files_layout = FilesLayout::new(self.logical_client_width());
        let pairing_layout = PairingLayout::new(self.logical_client_width());
        let settings_layout = SettingsLayout::new(self.logical_client_width());
        let shell = ShellLayout::new(self.logical_client_width());
        for (control, _) in &self.buttons {
            let id = unsafe { GetDlgCtrlID(*control) } as usize;
            let bounds = if id == ID_LANGUAGE_MENU {
                Some(shell.language_action)
            } else if self.page == Page::Phones && id == ID_PAIR {
                Some(phone_layout.pair_action)
            } else if self.page == Page::Status && id == ID_PAIR {
                Some(StatusLayout::new(self.logical_client_width()).pair_action)
            } else if self.page == Page::Settings && id == ID_SAVE_SETTINGS {
                Some(settings_layout.save_action)
            } else if self.page == Page::Settings && id == ID_REPAIR {
                Some(settings_layout.repair_action)
            } else if self.page == Page::Settings && id == ID_UPDATE_REPOSITORY {
                Some(settings_layout.update_repository)
            } else if self.page == Page::Settings && id == ID_UPDATE_HISTORY {
                Some(settings_layout.update_history)
            } else if self.page == Page::Settings && id == ID_UPDATE_ACTION {
                Some(settings_layout.update_action)
            } else if self.page == Page::Files && id == ID_CHANGE_RECEIVE_FOLDER {
                Some(files_layout.receive_action)
            } else if self.page == Page::Phones
                && (ID_UNPAIR_BASE..ID_UNPAIR_BASE + phone_layout.rows.len()).contains(&id)
            {
                Some(phone_layout.rows[id - ID_UNPAIR_BASE].action)
            } else {
                None
            };
            if let Some(bounds) = bounds {
                self.position_control(*control, bounds);
            }
        }
        match self.page {
            Page::Settings => self.position_control(self.name_edit, settings_layout.name_edit),
            Page::Files => {
                for (control, rect) in [
                    (self.file_destination, files_layout.destination),
                    (self.file_status, files_layout.status),
                    (self.file_failures, files_layout.failures),
                    (self.file_drop_hint, files_layout.drop_hint),
                    (self.file_security_note, files_layout.security_note),
                    (self.file_receive_root, files_layout.receive_root),
                ] {
                    self.position_control(control, rect);
                }
            }
            Page::Phones => {
                for (index, row) in phone_layout.rows.iter().enumerate() {
                    self.position_control(self.phone_names[index], row.name);
                    self.position_control(self.phone_statuses[index], row.status);
                }
            }
            Page::Pairing => {
                for (control, rect) in [
                    (self.pair_open_instruction, pairing_layout.open_instruction),
                    (self.pair_scan_instruction, pairing_layout.scan_instruction),
                    (self.pair_computer, pairing_layout.computer),
                    (self.pair_waiting, pairing_layout.waiting),
                    (self.pair_note, pairing_layout.note),
                ] {
                    self.position_control(control, rect);
                }
            }
            Page::Status => {}
        }
        unsafe {
            RedrawWindow(
                self.hwnd,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_NOERASE | RDW_ALLCHILDREN,
            );
        }
    }

    fn position_control(&self, control: HWND, bounds: ui_layout::Rect) {
        if control.is_null() {
            return;
        }
        unsafe {
            SetWindowPos(
                control,
                null_mut(),
                self.scale(bounds.left as i32),
                self.scale(bounds.top as i32),
                self.scale(bounds.width() as i32),
                self.scale((bounds.bottom - bounds.top) as i32),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub(super) fn rebuild_fonts(&mut self) {
        for font in [
            self.title_font,
            self.heading_font,
            self.navigation_font,
            self.body_font,
            self.icon_font,
        ] {
            if !font.is_null() {
                unsafe { DeleteObject(font) };
            }
        }
        self.title_font = create_font(self.scale(22), FW_SEMIBOLD as i32, "Segoe UI");
        self.heading_font = create_font(self.scale(17), FW_SEMIBOLD as i32, "Segoe UI");
        self.navigation_font = create_font(self.scale(15), FW_SEMIBOLD as i32, "Segoe UI");
        self.body_font = create_font(self.scale(14), FW_NORMAL as i32, "Segoe UI");
        self.icon_font = create_font(self.scale(20), FW_NORMAL as i32, "Segoe MDL2 Assets");
    }

    pub(super) fn rebuild_page(&mut self) {
        unsafe { SendMessageW(self.hwnd, WM_SETREDRAW, 0, 0) };
        for control in self.controls.drain(..) {
            unsafe { DestroyWindow(control) };
        }
        self.phone_ids.clear();
        self.buttons
            .retain(|(control, _)| self.shell_controls.contains(control));
        self.muted_controls.clear();
        self.qr = None;
        self.name_edit = null_mut();
        self.status_summary = null_mut();
        self.status_phone = null_mut();
        self.status_target = null_mut();
        self.status_address = null_mut();
        self.status_error = null_mut();
        self.phone_statuses.clear();
        self.phone_names.clear();
        self.file_destination = null_mut();
        self.file_status = null_mut();
        self.file_failures = null_mut();
        self.file_drop_hint = null_mut();
        self.file_security_note = null_mut();
        self.file_receive_root = null_mut();
        self.file_choose = null_mut();
        self.file_folder = null_mut();
        self.file_cancel = null_mut();
        self.pair_open_instruction = null_mut();
        self.pair_scan_instruction = null_mut();
        self.pair_computer = null_mut();
        self.pair_waiting = null_mut();
        self.pair_note = null_mut();
        self.service_status = null_mut();
        self.auto_start = null_mut();
        self.show_floating = null_mut();
        self.update_status = null_mut();
        self.update_progress = null_mut();
        self.update_action = null_mut();
        self.update_repository = null_mut();
        self.update_history = null_mut();
        match self.page {
            Page::Status => self.build_status(),
            Page::Files => self.build_files(),
            Page::Phones => self.build_phones(),
            Page::Settings => self.build_settings(),
            Page::Pairing => self.build_pairing(),
        }
        self.refresh_shell();
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

    pub(super) fn build_files(&mut self) {
        let snapshot = self.state.snapshot();
        let layout = FilesLayout::new(self.logical_client_width());
        self.text(
            tr("文件传输", "File transfer"),
            220,
            28,
            300,
            34,
            self.title_font,
        );
        self.text(
            tr("发送到手机", "Send to phone"),
            220,
            96,
            260,
            30,
            self.heading_font,
        );
        let destination = snapshot
            .status
            .connected_phone
            .as_deref()
            .unwrap_or(tr("手机未连接", "Phone is not connected"));
        self.file_destination = self.muted_text(
            destination,
            layout.destination.left as i32,
            layout.destination.top as i32,
            layout.destination.width() as i32,
            26,
            self.body_font,
        );
        let transfer_status = snapshot.file_transfer.as_ref().map_or_else(
            || {
                tr(
                    "选择文件后，手机只需确认一次",
                    "The phone confirms once per transfer",
                )
                .to_owned()
            },
            file_transfer_status,
        );
        self.file_status = self.muted_text(
            &transfer_status,
            layout.status.left as i32,
            layout.status.top as i32,
            layout.status.width() as i32,
            42,
            self.body_font,
        );
        let active = snapshot
            .file_transfer
            .as_ref()
            .is_some_and(|transfer| transfer.active());
        self.file_choose = self.owner_button(
            tr("选择文件", "Choose files"),
            ID_CHOOSE_FILES,
            220,
            205,
            128,
            38,
            ButtonKind::Secondary,
        );
        self.file_folder = self.owner_button(
            tr("选择文件夹", "Choose folder"),
            ID_CHOOSE_FOLDER,
            360,
            205,
            128,
            38,
            ButtonKind::Secondary,
        );
        self.file_cancel = self.owner_button(
            tr("取消传输", "Cancel transfer"),
            ID_CANCEL_FILE_TRANSFER,
            220,
            205,
            112,
            38,
            ButtonKind::Secondary,
        );
        unsafe {
            ShowWindow(self.file_choose, if active { SW_HIDE } else { SW_SHOW });
            ShowWindow(self.file_folder, if active { SW_HIDE } else { SW_SHOW });
            ShowWindow(self.file_cancel, if active { SW_SHOW } else { SW_HIDE });
        }
        self.file_failures = self.muted_text(
            &snapshot
                .file_transfer
                .as_ref()
                .filter(|transfer| !transfer.failed_paths.is_empty())
                .map_or_else(String::new, |transfer| {
                    format!(
                        "{}：{}",
                        tr("失败文件", "Failed files"),
                        transfer.failed_paths.join(" · ")
                    )
                }),
            layout.failures.left as i32,
            layout.failures.top as i32,
            layout.failures.width() as i32,
            42,
            self.body_font,
        );
        self.file_drop_hint = self.text(
            tr(
                "也可以把文件或文件夹拖到此窗口",
                "You can also drop files or folders here",
            ),
            layout.drop_hint.left as i32,
            layout.drop_hint.top as i32,
            layout.drop_hint.width() as i32,
            30,
            self.body_font,
        );
        self.file_security_note = self.muted_text(
            tr(
                "传输经过设备绑定密钥保护，支持断点继续。",
                "Transfers are protected by the paired device identity and can resume.",
            ),
            layout.security_note.left as i32,
            layout.security_note.top as i32,
            layout.security_note.width() as i32,
            48,
            self.body_font,
        );
        self.text(
            tr("手机文件保存到", "Files from phone are saved to"),
            220,
            420,
            260,
            26,
            self.heading_font,
        );
        let receive_root = self
            .state
            .file_receive_root()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| tr("接收位置不可用", "Receive folder unavailable").to_owned());
        self.file_receive_root = self.muted_text(
            &receive_root,
            layout.receive_root.left as i32,
            layout.receive_root.top as i32,
            layout.receive_root.width() as i32,
            44,
            self.body_font,
        );
        self.owner_button(
            tr("更改位置", "Change"),
            ID_CHANGE_RECEIVE_FOLDER,
            layout.receive_action.left as i32,
            layout.receive_action.top as i32,
            100,
            36,
            ButtonKind::Secondary,
        );
    }

    pub(super) fn build_status(&mut self) {
        let snapshot = self.state.snapshot();
        let layout = StatusLayout::new(self.logical_client_width());
        self.status_summary = self.text(
            &snapshot.status.summary,
            layout.summary.left as i32,
            layout.summary.top as i32,
            layout.summary.width() as i32,
            (layout.summary.bottom - layout.summary.top) as i32,
            self.title_font,
        );
        self.status_phone = self.label_value(
            tr("手机", "Phone"),
            snapshot
                .status
                .connected_phone
                .as_deref()
                .unwrap_or(tr("尚未连接", "Not connected")),
            105,
        );
        self.label_value(
            tr("输入状态", "Input status"),
            tr("等待手机输入", "Waiting for phone input"),
            151,
        );
        self.status_target = self.label_value(
            tr("输入位置", "Input location"),
            snapshot
                .status
                .target_name
                .as_deref()
                .unwrap_or(tr("尚未选择", "Not selected")),
            205,
        );
        self.status_address = self.label_value(
            tr("连接地址", "Address"),
            &format!("{}:{PORT}", self.host),
            259,
        );
        self.status_error = self.muted_text(
            snapshot.status.last_error.as_deref().unwrap_or_default(),
            220,
            302,
            470,
            42,
            self.body_font,
        );
        self.owner_button(
            tr("绑定手机", "Pair phone"),
            ID_PAIR,
            layout.pair_action.left as i32,
            layout.pair_action.top as i32,
            layout.pair_action.width() as i32,
            (layout.pair_action.bottom - layout.pair_action.top) as i32,
            ButtonKind::Secondary,
        );
    }

    pub(super) fn build_phones(&mut self) {
        let snapshot = self.state.snapshot();
        let layout = PhonesLayout::new(self.logical_client_width(), snapshot.phones.len());
        self.text(
            tr("已绑定手机", "Paired phones"),
            layout.title.left as i32,
            layout.title.top as i32,
            layout.title.width() as i32,
            (layout.title.bottom - layout.title.top) as i32,
            self.title_font,
        );
        self.owner_button(
            tr("绑定手机", "Pair phone"),
            ID_PAIR,
            layout.pair_action.left as i32,
            layout.pair_action.top as i32,
            layout.pair_action.width() as i32,
            (layout.pair_action.bottom - layout.pair_action.top) as i32,
            ButtonKind::Secondary,
        );
        if snapshot.phones.is_empty() {
            self.muted_text(
                tr("还没有绑定手机", "No phones paired"),
                layout.empty_message.left as i32,
                layout.empty_message.top as i32,
                layout.empty_message.width() as i32,
                (layout.empty_message.bottom - layout.empty_message.top) as i32,
                self.body_font,
            );
        }
        for (index, (phone_id, phone)) in snapshot.phones.iter().enumerate() {
            let row = layout.rows[index];
            let name_control = self.text(
                &phone.phone_name,
                row.name.left as i32,
                row.name.top as i32,
                row.name.width() as i32,
                (row.name.bottom - row.name.top) as i32,
                self.heading_font,
            );
            self.phone_names.push(name_control);
            let connected =
                snapshot.status.connected_phone.as_deref() == Some(phone.phone_name.as_str());
            let status = if connected {
                tr("已连接", "Connected").to_owned()
            } else if phone.last_connected.is_some() {
                format!(
                    "{} · {}",
                    tr("未连接", "Offline"),
                    tr("最近连接过", "Connected previously")
                )
            } else {
                format!(
                    "{} · {}",
                    tr("未连接", "Offline"),
                    tr("尚未连接过", "Never connected")
                )
            };
            let status_control = self.muted_text(
                &status,
                row.status.left as i32,
                row.status.top as i32,
                row.status.width() as i32,
                (row.status.bottom - row.status.top) as i32,
                self.body_font,
            );
            self.phone_statuses.push(status_control);
            self.owner_button(
                tr("解除绑定", "Unpair"),
                ID_UNPAIR_BASE + index,
                row.action.left as i32,
                row.action.top as i32,
                row.action.width() as i32,
                (row.action.bottom - row.action.top) as i32,
                ButtonKind::Secondary,
            );
            self.phone_ids.push(phone_id.clone());
        }
    }

    pub(super) fn build_settings(&mut self) {
        let snapshot = self.state.snapshot();
        let layout = SettingsLayout::new(self.logical_client_width());
        self.text(
            tr("设置", "Settings"),
            layout.shell.title.left as i32,
            layout.shell.title.top as i32,
            layout.shell.title.width() as i32,
            (layout.shell.title.bottom - layout.shell.title.top) as i32,
            self.title_font,
        );
        self.text(tr("常规", "General"), 220, 92, 120, 26, self.heading_font);
        self.text(
            tr("电脑名称", "Computer name"),
            220,
            134,
            125,
            28,
            self.body_font,
        );
        self.name_edit = self.control(
            "EDIT",
            &snapshot.pc_name,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            ID_NAME,
            layout.name_edit.left as i32,
            layout.name_edit.top as i32,
            layout.name_edit.width() as i32,
            (layout.name_edit.bottom - layout.name_edit.top) as i32,
            self.body_font,
        );
        self.auto_start_checked = settings::auto_start_enabled();
        self.auto_start = self.owner_button(
            tr("开机启动", "Start with Windows"),
            ID_AUTO_START,
            220,
            174,
            330,
            34,
            ButtonKind::Checkbox,
        );
        self.floating_enabled_checked = settings::floating_enabled();
        self.show_floating = self.owner_button(
            tr("显示悬浮球", "Show floating ball"),
            ID_SHOW_FLOATING,
            220,
            214,
            220,
            34,
            ButtonKind::Checkbox,
        );
        self.owner_button(
            tr("保存设置", "Save settings"),
            ID_SAVE_SETTINGS,
            layout.save_action.left as i32,
            layout.save_action.top as i32,
            layout.save_action.width() as i32,
            (layout.save_action.bottom - layout.save_action.top) as i32,
            ButtonKind::Secondary,
        );

        self.text(
            tr("输入服务", "Input service"),
            220,
            272,
            140,
            26,
            self.heading_font,
        );
        let service = if snapshot.injector_ready {
            tr("正常运行", "Running")
        } else {
            tr("输入服务不可用", "Input unavailable")
        };
        self.service_status = self.text(service, 246, 308, 180, 28, self.body_font);
        self.owner_button(
            tr("修复输入服务", "Repair input"),
            ID_REPAIR,
            layout.repair_action.left as i32,
            layout.repair_action.top as i32,
            layout.repair_action.width() as i32,
            (layout.repair_action.bottom - layout.repair_action.top) as i32,
            ButtonKind::Secondary,
        );
        self.text(
            tr("版本与更新", "Version & updates"),
            220,
            370,
            180,
            26,
            self.heading_font,
        );
        self.text(
            &format!(
                "{} {} · Windows x64",
                tr("当前版本", "Current version"),
                env!("CARGO_PKG_VERSION")
            ),
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
            tr("GitHub 仓库", "GitHub"),
            ID_UPDATE_REPOSITORY,
            layout.update_repository.left as i32,
            layout.update_repository.top as i32,
            layout.update_repository.width() as i32,
            (layout.update_repository.bottom - layout.update_repository.top) as i32,
            ButtonKind::Secondary,
        );
        self.update_history = self.owner_button(
            tr("查看更新", "Release history"),
            ID_UPDATE_HISTORY,
            layout.update_history.left as i32,
            layout.update_history.top as i32,
            layout.update_history.width() as i32,
            (layout.update_history.bottom - layout.update_history.top) as i32,
            ButtonKind::Secondary,
        );
        self.update_action = self.owner_button(
            tr("检查更新", "Check now"),
            ID_UPDATE_ACTION,
            layout.update_action.left as i32,
            layout.update_action.top as i32,
            layout.update_action.width() as i32,
            (layout.update_action.bottom - layout.update_action.top) as i32,
            ButtonKind::Secondary,
        );
        self.muted_text(
            tr(
                "检查更新会连接 GitHub。",
                "Checking for updates connects to GitHub.",
            ),
            220,
            518,
            500,
            34,
            self.body_font,
        );
        self.refresh_update_controls();
    }

    pub(super) fn build_pairing(&mut self) {
        let layout = PairingLayout::new(self.logical_client_width());
        self.owner_button(
            "",
            ID_NAV_PHONES,
            layout.back_action.left as i32,
            layout.back_action.top as i32,
            layout.back_action.width() as i32,
            (layout.back_action.bottom - layout.back_action.top) as i32,
            ButtonKind::Back,
        );
        self.text(
            tr("绑定手机", "Pair phone"),
            layout.title.left as i32,
            layout.title.top as i32,
            layout.title.width() as i32,
            (layout.title.bottom - layout.title.top) as i32,
            self.heading_font,
        );
        self.pair_open_instruction = self.text(
            tr("在手机上打开说写，", "Open FlowType on your phone"),
            layout.open_instruction.left as i32,
            layout.open_instruction.top as i32,
            layout.open_instruction.width() as i32,
            (layout.open_instruction.bottom - layout.open_instruction.top) as i32,
            self.heading_font,
        );
        self.pair_scan_instruction = self.text(
            tr("扫描此二维码", "Scan this QR code"),
            layout.scan_instruction.left as i32,
            layout.scan_instruction.top as i32,
            layout.scan_instruction.width() as i32,
            (layout.scan_instruction.bottom - layout.scan_instruction.top) as i32,
            self.heading_font,
        );
        let pc_name = self.state.snapshot().pc_name;
        self.pair_computer = self.text(
            &format!("{}{pc_name}", tr("此电脑：", "Computer: ")),
            layout.computer.left as i32,
            layout.computer.top as i32,
            layout.computer.width() as i32,
            (layout.computer.bottom - layout.computer.top) as i32,
            self.body_font,
        );
        self.pair_waiting = self.text(
            tr("等待手机扫描", "Waiting for scan"),
            layout.waiting.left as i32,
            layout.waiting.top as i32,
            layout.waiting.width() as i32,
            (layout.waiting.bottom - layout.waiting.top) as i32,
            self.body_font,
        );
        self.pair_note = self.muted_text(
            tr(
                "二维码仅用于本次绑定，绑定成功后立即失效",
                "This QR code expires after pairing",
            ),
            layout.note.left as i32,
            layout.note.top as i32,
            layout.note.width() as i32,
            (layout.note.bottom - layout.note.top) as i32,
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
                    self.text(
                        tr("二维码生成失败", "Could not create QR code"),
                        layout.qr_origin.0 as i32,
                        140,
                        250,
                        30,
                        self.body_font,
                    );
                }
            },
            Err(_) => {
                self.text(
                    tr(
                        "无法开始绑定，请重新打开此页面",
                        "Could not start pairing. Reopen this page.",
                    ),
                    layout.qr_origin.0 as i32,
                    140,
                    430,
                    30,
                    self.body_font,
                );
            }
        }
    }

    pub(super) fn label_value(&mut self, label: &str, value: &str, y: i32) -> HWND {
        let shell = ShellLayout::new(self.logical_client_width());
        self.muted_text(label, shell.content_left as i32, y, 120, 28, self.body_font);
        self.text(
            value,
            (shell.content_left + 150.0) as i32,
            y,
            (shell.content_right - shell.content_left - 150.0) as i32,
            28,
            self.body_font,
        )
    }

    pub(super) fn text(
        &mut self,
        value: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        font: HFONT,
    ) -> HWND {
        self.control(
            "STATIC",
            value,
            WS_CHILD | WS_VISIBLE | SS_NOPREFIX,
            0,
            x,
            y,
            width,
            height,
            font,
        )
    }

    pub(super) fn muted_text(
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
    pub(super) fn owner_button(
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
        if !control.is_null() {
            track_button_hover(control);
        }
        control
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn control(
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
}

pub(super) fn file_transfer_status(
    transfer: &crate::file_transfer_server::FileTransferSnapshot,
) -> String {
    use flowtype_core::file_transfer::{BatchState, TransferDirection};
    let action = match (transfer.direction, transfer.state) {
        (TransferDirection::ComputerToPhone, BatchState::WaitingConfirmation) => {
            tr("等待手机确认接收", "Waiting for phone approval")
        }
        (TransferDirection::ComputerToPhone, BatchState::Completed) => {
            tr("已发送到手机", "Sent to phone")
        }
        (TransferDirection::PhoneToComputer, BatchState::Completed) => {
            tr(r"已保存到“下载\FlowType”", r"Saved to Downloads\FlowType")
        }
        (_, BatchState::PartialFailure) => tr("部分文件传输失败", "Some files failed"),
        (_, BatchState::Canceled) => tr("传输已取消", "Transfer canceled"),
        (TransferDirection::PhoneToComputer, _) => tr(
            "正在接收手机文件，可断点继续",
            "Receiving from phone; resumable",
        ),
        (TransferDirection::ComputerToPhone, _) => {
            tr("正在发送到手机，可断点继续", "Sending to phone; resumable")
        }
    };
    format!(
        "{} · {} / {} · {} / {}{}",
        action,
        format_file_size(transfer.completed_bytes),
        format_file_size(transfer.total_bytes),
        transfer.completed_entries,
        transfer.total_entries,
        if transfer.failed_entries > 0 {
            if is_chinese() {
                format!(" · {} 个失败", transfer.failed_entries)
            } else {
                format!(" · {} failed", transfer.failed_entries)
            }
        } else {
            String::new()
        },
    )
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
