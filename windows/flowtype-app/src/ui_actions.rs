use super::*;

impl UiContext {
    pub(super) fn set_page(&mut self, page: Page) {
        if self.page == Page::Pairing && page != Page::Pairing {
            self.state.cancel_pairing();
        }
        self.page = page;
        self.rebuild_page();
    }

    pub(super) fn create_language_button(&mut self) {
        let bounds = ShellLayout::new(self.logical_client_width()).language_action;
        self.language_button = self.owner_button(
            "语言 / Language",
            ID_LANGUAGE_MENU,
            bounds.left as i32,
            bounds.top as i32,
            bounds.width() as i32,
            (bounds.bottom - bounds.top) as i32,
            ButtonKind::Language,
        );
        self.language_tooltip_text = wide("语言 / Language");
        let tooltip = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                null(),
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                self.hwnd,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if tooltip.is_null() {
            return;
        }
        let mut tool = TTTOOLINFOW {
            cbSize: size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd: self.hwnd,
            uId: self.language_button as usize,
            lpszText: self.language_tooltip_text.as_mut_ptr(),
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                tooltip,
                TTM_ADDTOOLW,
                0,
                (&mut tool as *mut TTTOOLINFOW) as isize,
            );
        }
        self.controls.push(tooltip);
    }

    pub(super) fn show_language_menu(&mut self) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }
        let selected = language_choice();
        unsafe {
            append_checked_menu(
                menu,
                ID_LANGUAGE_SYSTEM,
                "跟随系统 / System",
                selected == LanguageChoice::System,
            );
            append_checked_menu(
                menu,
                ID_LANGUAGE_CHINESE,
                "简体中文",
                selected == LanguageChoice::Chinese,
            );
            append_checked_menu(
                menu,
                ID_LANGUAGE_ENGLISH,
                "English",
                selected == LanguageChoice::English,
            );
            let mut button_rect: RECT = zeroed();
            GetWindowRect(self.language_button, &mut button_rect);
            SetForegroundWindow(self.hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RIGHTALIGN | TPM_TOPALIGN | TPM_RETURNCMD,
                button_rect.right,
                button_rect.bottom,
                0,
                self.hwnd,
                null(),
            );
            DestroyMenu(menu);
            let choice = match command as usize {
                ID_LANGUAGE_SYSTEM => Some(LanguageChoice::System),
                ID_LANGUAGE_CHINESE => Some(LanguageChoice::Chinese),
                ID_LANGUAGE_ENGLISH => Some(LanguageChoice::English),
                _ => None,
            };
            if let Some(choice) = choice {
                self.apply_language(choice);
            }
        }
    }

    pub(super) fn apply_language(&mut self, choice: LanguageChoice) {
        if set_language_choice(choice).is_err() {
            message_box(
                self.hwnd,
                tr("无法保存语言设置", "Could not save the language setting"),
                product_name(),
                MB_OK | MB_ICONERROR,
            );
            return;
        }
        self.state.language_changed();
        unsafe { SetWindowTextW(self.hwnd, wide(product_name()).as_ptr()) };
        self.rebuild_page();
    }

    pub(super) fn save_settings(&mut self) {
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
            (true, tr("设置已保存", "Settings saved"))
        } else {
            (
                false,
                tr(
                    "设置保存失败，请检查电脑名称",
                    "Could not save settings. Check the computer name.",
                ),
            )
        };
        self.rebuild_page();
        self.show_save_notice(success, text);
    }

    pub(super) fn show_save_notice(&mut self, success: bool, text: &str) {
        self.save_notice = Some(SaveNotice {
            success,
            text: text.to_owned(),
        });
        unsafe { SetTimer(self.hwnd, NOTICE_TIMER, 2600, None) };
        unsafe { InvalidateRect(self.hwnd, null(), 1) };
    }

    pub(super) fn apply_floating_visibility(&mut self) {
        if self.floating_enabled_checked {
            if self.ball_hwnd.is_null() {
                self.ball_hwnd = ui_floating::create(self.state.clone(), self.hwnd);
            }
        } else if !self.ball_hwnd.is_null() {
            unsafe { DestroyWindow(self.ball_hwnd) };
            self.ball_hwnd = null_mut();
        }
    }

    pub(super) fn repair_injector(&mut self) {
        let result = self.state.repair_injector();
        message_box(
            self.hwnd,
            if result.is_ok() {
                tr("输入服务已恢复", "Input service repaired")
            } else {
                tr(
                    "无法启动输入服务，请重新运行安装程序",
                    "Could not start the input service. Run the installer again.",
                )
            },
            product_name(),
            if result.is_ok() {
                MB_OK | MB_ICONINFORMATION
            } else {
                MB_OK | MB_ICONERROR
            },
        );
        self.rebuild_page();
    }

    pub(super) fn confirm_unpair(&mut self, index: usize) {
        let Some(phone_id) = self.phone_ids.get(index).cloned() else {
            return;
        };
        let snapshot = self.state.snapshot();
        let name = snapshot
            .phones
            .iter()
            .find(|(id, _)| id == &phone_id)
            .map(|(_, phone)| phone.phone_name.as_str())
            .unwrap_or(tr("这部手机", "this phone"));
        if message_box(
            self.hwnd,
            &if is_chinese() {
                format!("解绑“{name}”？解绑后需要重新扫描二维码。")
            } else {
                format!("Unpair \"{name}\"? You will need to scan the QR code again.")
            },
            tr("解绑手机", "Unpair phone"),
            MB_YESNO | MB_ICONWARNING,
        ) == IDYES
        {
            let _ = self.state.unpair_phone(&phone_id);
            self.rebuild_page();
        }
    }

    pub(super) fn update_from_state(&mut self) {
        if self.page == Page::Pairing && self.state.current_pairing_uri(self.host).is_none() {
            self.page = Page::Status;
            self.rebuild_page();
            return;
        }
        let snapshot = self.state.snapshot();
        if self.page == Page::Phones && self.phone_statuses.len() != snapshot.phones.len() {
            self.rebuild_page();
            return;
        }
        match self.page {
            Page::Status => {
                set_control_text(self.status_summary, &snapshot.status.summary);
                set_control_text(
                    self.status_phone,
                    snapshot
                        .status
                        .connected_phone
                        .as_deref()
                        .unwrap_or(tr("尚未连接", "Not connected")),
                );
                set_control_text(
                    self.status_target,
                    snapshot
                        .status
                        .target_name
                        .as_deref()
                        .unwrap_or(tr("尚未选择", "Not selected")),
                );
                set_control_text(self.status_address, &format!("{}:{PORT}", self.host));
                set_control_text(
                    self.status_error,
                    snapshot.status.last_error.as_deref().unwrap_or_default(),
                );
            }
            Page::Phones => {
                for (index, (_, phone)) in snapshot.phones.iter().enumerate() {
                    let connected = snapshot.status.connected_phone.as_deref()
                        == Some(phone.phone_name.as_str());
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
                    set_control_text(self.phone_statuses[index], &status);
                }
            }
            Page::Settings => {
                set_control_text(
                    self.service_status,
                    if snapshot.injector_ready {
                        tr("正常运行", "Running")
                    } else {
                        tr("输入服务不可用", "Input unavailable")
                    },
                );
                self.refresh_update_controls();
            }
            Page::Pairing => {}
        }
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
        self.update_tray();
        ui_floating::refresh(self.ball_hwnd);
    }

    pub(super) fn refresh_update_controls(&mut self) {
        if self.page != Page::Settings || self.update_status.is_null() {
            self.update_tray();
            return;
        }
        let snapshot = self.state.snapshot().update;
        let status = if snapshot.action == update::UpdateAction::Install
            && self.state.update_install_blocked()
        {
            tr("输入结束后可安装", "Available after input ends").to_owned()
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
            RedrawWindow(
                self.hwnd,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_NOERASE | RDW_ALLCHILDREN,
            );
        }
        self.update_tray();
    }

    pub(super) fn handle_update_action(&mut self) {
        let action = self.state.snapshot().update.action;
        if action == update::UpdateAction::Install {
            if self.state.update_install_blocked() {
                self.show_save_notice(
                    false,
                    tr(
                        "请先完成当前输入，再安装更新",
                        "Finish the current input before installing",
                    ),
                );
                return;
            }
            if self.state.install_update().is_ok() {
                unsafe { DestroyWindow(self.hwnd) };
            }
        } else {
            self.state.perform_update_action(action);
        }
    }
}
