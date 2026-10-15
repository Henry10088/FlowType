use super::*;

impl UiContext {
    pub(super) fn draw_button(&self, item: &DRAWITEMSTRUCT) {
        let Some((_, kind)) = self
            .buttons
            .iter()
            .find(|(window, _)| *window == item.hwndItem)
        else {
            return;
        };
        let pressed = item.itemState & ODS_SELECTED != 0;
        let focused = item.itemState & ODS_FOCUS != 0;
        let disabled = item.itemState & ODS_DISABLED != 0;
        let visual = ButtonVisual {
            pressed,
            focused,
            disabled,
            hovered: button_hovered(item.hwndItem),
        };
        let mut rect = item.rcItem;
        let text = window_text(item.hwndItem);
        unsafe { SetBkMode(item.hDC, TRANSPARENT as i32) };

        match kind {
            ButtonKind::Navigation(target) => {
                let selected = self.selected_navigation() == *target;
                fill(item.hDC, &rect, COLOR_SIDEBAR);
                let selection = RECT {
                    left: rect.left + self.scale(NAV_INSET),
                    right: rect.right - self.scale(NAV_INSET),
                    top: rect.top + self.scale(2),
                    bottom: rect.bottom - self.scale(2),
                };
                if selected {
                    fill_round_rect(
                        item.hDC,
                        selection,
                        COLOR_TEAL_PALE,
                        self.scale(CORNER_RADIUS),
                    );
                } else if visual.hovered || pressed {
                    fill_round_rect(
                        item.hDC,
                        selection,
                        if pressed { COLOR_PRESSED } else { COLOR_HOVER },
                        self.scale(CORNER_RADIUS),
                    );
                }
                if selected {
                    let marker = RECT {
                        left: selection.left,
                        right: selection.left + self.scale(3),
                        top: selection.top + self.scale(9),
                        bottom: selection.bottom - self.scale(9),
                    };
                    fill_round_rect(item.hDC, marker, COLOR_TEAL, self.scale(2));
                }
                if focused && !visual.hovered {
                    outline_round_rect(
                        item.hDC,
                        selection,
                        COLOR_TEAL,
                        self.scale(CORNER_RADIUS * 2),
                    );
                }
                let icon_rect = RECT {
                    left: self.scale(26),
                    right: self.scale(54),
                    ..rect
                };
                let icon_color = if selected { COLOR_TEAL } else { COLOR_MUTED };
                if *target == Page::Settings {
                    draw_label(
                        item.hDC,
                        "\u{e713}",
                        icon_rect,
                        self.icon_font,
                        icon_color,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                } else {
                    draw_navigation_icon(item.hDC, *target, icon_rect, icon_color, self.scale(1));
                }
                rect.left = self.scale(65);
                draw_label(
                    item.hDC,
                    &text,
                    rect,
                    self.navigation_font,
                    if disabled {
                        COLOR_MUTED
                    } else if selected {
                        COLOR_TEAL
                    } else {
                        COLOR_MUTED
                    },
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Secondary => {
                let style = if matches!(
                    item.CtlID as usize,
                    ID_PAIR | ID_SAVE_SETTINGS | ID_CHOOSE_FILES
                ) {
                    ActionStyle::Primary
                } else {
                    ActionStyle::Secondary
                };
                draw_action(item.hDC, rect, style, visual, self.scale(CORNER_RADIUS));
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
                        if style == ActionStyle::Primary {
                            COLOR_WHITE
                        } else {
                            COLOR_TEAL
                        },
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                    text_rect.left += self.scale(26);
                }
                draw_label(
                    item.hDC,
                    &text,
                    text_rect,
                    self.body_font,
                    action_text_color(style, disabled),
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Back => {
                draw_action(
                    item.hDC,
                    rect,
                    ActionStyle::Subtle,
                    visual,
                    self.scale(CORNER_RADIUS),
                );
                draw_label(
                    item.hDC,
                    "←",
                    RECT {
                        left: rect.left,
                        right: rect.left + self.scale(30),
                        ..rect
                    },
                    self.icon_font,
                    COLOR_TEAL,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                draw_label(
                    item.hDC,
                    &text,
                    RECT {
                        left: rect.left + self.scale(28),
                        ..rect
                    },
                    self.body_font,
                    COLOR_TEAL,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Checkbox => {
                let checked = if item.CtlID as usize == ID_SHOW_FLOATING {
                    self.floating_enabled_checked
                } else {
                    self.auto_start_checked
                };
                fill(item.hDC, &rect, COLOR_WHITE);
                if visual.hovered {
                    fill_round_rect(item.hDC, rect, COLOR_HOVER, self.scale(CORNER_RADIUS));
                }
                let box_size = self.scale(20);
                let top = (rect.top + rect.bottom - box_size) / 2;
                let box_rect = RECT {
                    left: rect.right - self.scale(38),
                    top,
                    right: rect.right - self.scale(3),
                    bottom: top + box_size,
                };
                fill_round_rect(
                    item.hDC,
                    box_rect,
                    if checked { COLOR_TEAL } else { COLOR_DISABLED },
                    self.scale(11),
                );
                let knob_left = if checked {
                    box_rect.right - self.scale(18)
                } else {
                    box_rect.left + self.scale(2)
                };
                fill_ellipse(
                    item.hDC,
                    RECT {
                        left: knob_left,
                        top: top + self.scale(2),
                        right: knob_left + self.scale(16),
                        bottom: top + self.scale(18),
                    },
                    COLOR_WHITE,
                );
                rect.right = box_rect.left - self.scale(12);
                draw_label(
                    item.hDC,
                    &text,
                    rect,
                    self.body_font,
                    COLOR_TEXT,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::Language => {
                draw_action(
                    item.hDC,
                    rect,
                    ActionStyle::Secondary,
                    visual,
                    self.scale(CORNER_RADIUS),
                );
                draw_label(
                    item.hDC,
                    "\u{e774}",
                    rect,
                    self.icon_font,
                    COLOR_TEAL,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::WindowMinimize | ButtonKind::WindowClose => {
                fill(
                    item.hDC,
                    &rect,
                    if visual.hovered {
                        COLOR_HOVER
                    } else {
                        COLOR_WHITE
                    },
                );
                draw_label(
                    item.hDC,
                    if matches!(kind, ButtonKind::WindowClose) {
                        "×"
                    } else {
                        "−"
                    },
                    rect,
                    self.navigation_font,
                    COLOR_MUTED,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            ButtonKind::FileTab(tab) => {
                fill(item.hDC, &rect, COLOR_WHITE);
                let selected = *tab == self.file_tab;
                draw_label(
                    item.hDC,
                    &text,
                    rect,
                    self.body_font,
                    if selected { COLOR_TEAL } else { COLOR_MUTED },
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
                if selected {
                    fill(
                        item.hDC,
                        &RECT {
                            left: rect.left,
                            right: rect.right - self.scale(6),
                            top: rect.bottom - self.scale(2),
                            bottom: rect.bottom,
                        },
                        COLOR_TEAL,
                    );
                }
                if focused {
                    outline_round_rect(item.hDC, rect, COLOR_TEAL, self.scale(7));
                }
            }
        }
        if focused && matches!(kind, ButtonKind::Checkbox) {
            let text_width = measure_text_width(item.hDC, &text, self.body_font);
            let text_height = self.scale(20);
            let focus = RECT {
                left: rect.left - self.scale(2),
                top: (rect.top + rect.bottom - text_height) / 2,
                right: rect.left + text_width + self.scale(2),
                bottom: (rect.top + rect.bottom + text_height) / 2,
            };
            outline_round_rect(item.hDC, focus, COLOR_TEAL, self.scale(CORNER_RADIUS * 2));
        }
    }

    pub(super) fn selected_navigation(&self) -> Page {
        if self.page == Page::Pairing {
            Page::Phones
        } else {
            self.page
        }
    }

    pub(super) fn paint(&mut self) {
        let mut paint: PAINTSTRUCT = unsafe { zeroed() };
        let output_dc = unsafe { BeginPaint(self.hwnd, &mut paint) };
        let mut client: RECT = unsafe { zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut client) };
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32;
        let logical_height = client.bottom as f32 * 96.0 / dpi;
        let panels = if self.page == Page::Files && self.file_tab == FileTab::Received {
            vec![ui_layout::Rect {
                left: 244.0,
                top: 210.0,
                right: ShellLayout::new(self.logical_client_width()).content_right,
                bottom: 326.0,
            }]
        } else {
            page_panels(self.page, self.logical_client_width(), self.phone_ids.len())
        };
        #[cfg(debug_assertions)]
        let simulate_failure = std::mem::take(&mut self.simulate_paint_error);
        {
            let result = self.direct2d.as_ref().map(|painter| {
                #[cfg(debug_assertions)]
                if simulate_failure {
                    return Err(windows::core::Error::from(
                        windows::Win32::Foundation::D2DERR_RECREATE_TARGET,
                    ));
                }
                match self.page {
                    Page::Status => painter.paint_status(
                        logical_height,
                        &StatusLayout::new(self.logical_client_width()),
                        &panels,
                        self.save_notice.as_ref(),
                    ),
                    Page::Files => painter.paint_files(
                        logical_height,
                        self.file_tab == FileTab::Send,
                        &panels,
                        self.save_notice.as_ref(),
                    ),
                    Page::Phones => {
                        let snapshot = self.state.snapshot();
                        let rows = snapshot
                            .phones
                            .iter()
                            .map(|(_, phone)| PhonePaintRow {
                                connected: snapshot.status.connected_phone.as_deref()
                                    == Some(phone.phone_name.as_str()),
                            })
                            .collect::<Vec<_>>();
                        painter.paint_phones(
                            logical_height,
                            &self.phones_layout(),
                            &rows,
                            &panels,
                            self.save_notice.as_ref(),
                        )
                    }
                    Page::Settings => painter.paint_settings(
                        logical_height,
                        &SettingsLayout::new(self.logical_client_width()),
                        self.state.snapshot().injector_ready,
                        unsafe { GetFocus() } == self.name_edit,
                        &panels,
                        self.save_notice.as_ref(),
                    ),
                    Page::Pairing => painter.paint_pairing(
                        logical_height,
                        &PairingLayout::new(self.logical_client_width()),
                        self.qr
                            .as_ref()
                            .map(|(width, modules)| (*width, modules.as_slice())),
                        &panels,
                        self.save_notice.as_ref(),
                    ),
                }
            });
            match result {
                Some(Ok(())) => {
                    unsafe { EndPaint(self.hwnd, &paint) };
                    return;
                }
                Some(Err(error)) => {
                    crate::diagnostics::log(format!("UI paint {:?} failed: {error}", self.page));
                    self.direct2d = None;
                }
                None => {}
            }
        }
        let buffer_dc = unsafe { CreateCompatibleDC(output_dc) };
        let bitmap = if !buffer_dc.is_null() {
            unsafe { CreateCompatibleBitmap(output_dc, client.right.max(1), client.bottom.max(1)) }
        } else {
            null_mut()
        };
        let old_bitmap = if !bitmap.is_null() {
            unsafe { SelectObject(buffer_dc, bitmap as HGDIOBJ) }
        } else {
            null_mut()
        };
        let dc = if old_bitmap.is_null() {
            output_dc
        } else {
            buffer_dc
        };
        fill(dc, &client, COLOR_WHITE);
        let sidebar = RECT {
            left: 0,
            top: self.scale(52),
            right: self.scale(202),
            bottom: client.bottom,
        };
        fill(dc, &sidebar, COLOR_SIDEBAR);
        draw_line(
            dc,
            0,
            self.scale(52),
            client.right,
            self.scale(52),
            COLOR_LINE,
        );
        draw_line(
            dc,
            self.scale(202),
            self.scale(52),
            self.scale(202),
            client.bottom,
            COLOR_LINE,
        );
        let brand = RECT {
            left: self.scale(21),
            top: self.scale(12),
            right: self.scale(48),
            bottom: self.scale(39),
        };
        fill_round_rect(dc, brand, COLOR_TEAL, self.scale(7));
        draw_label(
            dc,
            "\u{e70f}",
            brand,
            self.icon_font,
            COLOR_WHITE,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        draw_label(
            dc,
            tr("说写", "FlowType"),
            RECT {
                left: self.scale(57),
                top: self.scale(8),
                right: self.scale(137),
                bottom: self.scale(43),
            },
            self.heading_font,
            COLOR_TEXT,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        if is_chinese() {
            draw_label(
                dc,
                "FlowType",
                RECT {
                    left: self.scale(112),
                    top: self.scale(11),
                    right: self.scale(180),
                    bottom: self.scale(40),
                },
                self.body_font,
                COLOR_MUTED,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
        }
        draw_label(
            dc,
            tr("工作空间", "WORKSPACE"),
            RECT {
                left: self.scale(25),
                top: self.scale(76),
                right: self.scale(188),
                bottom: self.scale(100),
            },
            self.body_font,
            COLOR_MUTED,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        let foot = (client.bottom * 96 / unsafe { GetDpiForWindow(self.hwnd) }.max(96) as i32) - 50;
        draw_label(
            dc,
            tr("已加密连接", "Encrypted connection"),
            RECT {
                left: self.scale(24),
                top: self.scale(foot),
                right: self.scale(152),
                bottom: self.scale(foot + 34),
            },
            self.body_font,
            COLOR_MUTED,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        for panel in panels {
            draw_panel(
                dc,
                device_rect(panel, dpi as u32),
                self.scale(CORNER_RADIUS),
            );
        }

        match self.page {
            Page::Status => {
                let circle = device_rect(
                    StatusLayout::new(self.logical_client_width()).status_icon,
                    dpi as u32,
                );
                fill_round_rect(dc, circle, COLOR_TEAL_PALE, self.scale(12));
                draw_label(
                    dc,
                    "\u{e73e}",
                    circle,
                    self.icon_font,
                    COLOR_TEAL,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                let layout = StatusLayout::new(self.logical_client_width());
                for y in layout.separators {
                    draw_line(
                        dc,
                        self.scale(244),
                        self.scale(y as i32),
                        self.scale(layout.shell.content_right as i32),
                        self.scale(y as i32),
                        COLOR_LINE,
                    );
                }
            }
            Page::Phones => {
                let snapshot = self.state.snapshot();
                let layout = self.phones_layout();
                for ((_, phone), row) in snapshot.phones.iter().zip(layout.rows.iter()) {
                    draw_line(
                        dc,
                        self.scale(row.bounds.left as i32),
                        self.scale(row.bounds.bottom as i32),
                        self.scale(row.bounds.right as i32),
                        self.scale(row.bounds.bottom as i32),
                        COLOR_LINE,
                    );
                    let icon = row.icon;
                    fill_round_rect(
                        dc,
                        device_rect(icon, dpi as u32),
                        COLOR_HOVER,
                        self.scale(9),
                    );
                    draw_label(
                        dc,
                        "\u{e8ea}",
                        RECT {
                            left: self.scale(icon.left as i32),
                            top: self.scale(icon.top as i32),
                            right: self.scale(icon.right as i32),
                            bottom: self.scale(icon.bottom as i32),
                        },
                        self.icon_font,
                        COLOR_TEXT,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                    let dot = row.status_dot;
                    fill_ellipse(
                        dc,
                        RECT {
                            left: self.scale(dot.left as i32),
                            top: self.scale(dot.top as i32),
                            right: self.scale(dot.right as i32),
                            bottom: self.scale(dot.bottom as i32),
                        },
                        if snapshot.status.connected_phone.as_deref()
                            == Some(phone.phone_name.as_str())
                        {
                            COLOR_TEAL
                        } else {
                            COLOR_MUTED
                        },
                    );
                }
            }
            Page::Files => {
                if self.file_tab == FileTab::Send {
                    let center =
                        (244.0 + ShellLayout::new(self.logical_client_width()).content_right) / 2.0;
                    let icon = device_rect(
                        ui_layout::Rect::from_xywh(center - 20.0, 256.0, 41.0, 41.0),
                        dpi as u32,
                    );
                    fill_round_rect(dc, icon, COLOR_TEAL_PALE, self.scale(9));
                    draw_label(
                        dc,
                        "\u{e898}",
                        icon,
                        self.icon_font,
                        COLOR_TEAL,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                }
            }
            Page::Settings => {
                let layout = SettingsLayout::new(self.logical_client_width());
                for y in layout.separators {
                    draw_line(
                        dc,
                        self.scale(244),
                        self.scale(y as i32),
                        self.scale(layout.shell.content_right as i32),
                        self.scale(y as i32),
                        COLOR_LINE,
                    );
                }
                let field = SettingsLayout::new(self.logical_client_width()).name_frame;
                outline_round_rect(
                    dc,
                    device_rect(field, dpi as u32),
                    if unsafe { GetFocus() } == self.name_edit {
                        COLOR_TEAL
                    } else {
                        COLOR_LINE
                    },
                    self.scale(12),
                );
                let dot = device_rect(
                    SettingsLayout::new(self.logical_client_width()).service_status_dot,
                    dpi as u32,
                );
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
                let layout = PairingLayout::new(self.logical_client_width());
                outline_round_rect(
                    dc,
                    device_rect(
                        ui_layout::Rect::from_xywh(
                            layout.shell.content_left + 30.0,
                            194.0,
                            220.0,
                            243.0,
                        ),
                        dpi as u32,
                    ),
                    COLOR_LINE,
                    self.scale(14),
                );
                for (number, y) in [("1", 230.0), ("2", 315.0)] {
                    let bubble = device_rect(
                        ui_layout::Rect::from_xywh(layout.info_left, y, 24.0, 24.0),
                        dpi as u32,
                    );
                    fill_ellipse(dc, bubble, COLOR_TEAL_PALE);
                    draw_label(
                        dc,
                        number,
                        bubble,
                        self.body_font,
                        COLOR_TEAL,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                }
                let dot = device_rect(
                    PairingLayout::new(self.logical_client_width()).waiting_dot,
                    dpi as u32,
                );
                fill_ellipse(dc, dot, COLOR_TEAL);
            }
        }
        if let Some((width, modules)) = self.qr.as_ref() {
            let quiet = 4;
            let pairing = PairingLayout::new(self.logical_client_width());
            let origin_x = self.scale(pairing.qr_origin.0 as i32);
            let origin_y = self.scale(pairing.qr_origin.1 as i32);
            let total = self.scale(168);
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
                        let edge = |index: usize| {
                            (index as i32 * total + (*width as i32 + 8) / 2) / (*width as i32 + 8)
                        };
                        let left = origin_x + edge(column + quiet);
                        let top = origin_y + edge(row + quiet);
                        let rectangle = RECT {
                            left,
                            top,
                            right: origin_x + edge(column + quiet + 1),
                            bottom: origin_y + edge(row + quiet + 1),
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
            let notice_width = self.scale(222);
            let notice_right = self
                .scale(self.logical_client_width() as i32)
                .saturating_sub(self.scale(18));
            let toast = RECT {
                left: notice_right.saturating_sub(notice_width),
                top: self.scale(76),
                right: notice_right,
                bottom: self.scale(120),
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
                    0x00e4_e4f1
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
        unsafe {
            if !old_bitmap.is_null() {
                BitBlt(
                    output_dc,
                    0,
                    0,
                    client.right,
                    client.bottom,
                    dc,
                    0,
                    0,
                    SRCCOPY,
                );
                SelectObject(buffer_dc, old_bitmap);
            }
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !buffer_dc.is_null() {
                DeleteDC(buffer_dc);
            }
            EndPaint(self.hwnd, &paint);
        }
    }
}

fn draw_navigation_icon(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    page: Page,
    bounds: RECT,
    color: u32,
    unit: i32,
) {
    let x = (bounds.left + bounds.right) / 2;
    let y = (bounds.top + bounds.bottom) / 2;
    let s = |n: i32| n * unit;
    match page {
        Page::Status => {
            outline_round_rect(
                dc,
                RECT {
                    left: x - s(10),
                    right: x + s(10),
                    top: y - s(9),
                    bottom: y + s(5),
                },
                color,
                s(2),
            );
            draw_line(dc, x, y + s(5), x, y + s(10), color);
            draw_line(dc, x - s(5), y + s(10), x + s(5), y + s(10), color);
        }
        Page::Files => {
            draw_line(dc, x - s(9), y - s(5), x + s(9), y - s(5), color);
            draw_line(dc, x + s(9), y - s(5), x + s(5), y - s(9), color);
            draw_line(dc, x + s(9), y - s(5), x + s(5), y - s(1), color);
            draw_line(dc, x + s(9), y + s(5), x - s(9), y + s(5), color);
            draw_line(dc, x - s(9), y + s(5), x - s(5), y + s(1), color);
            draw_line(dc, x - s(9), y + s(5), x - s(5), y + s(9), color);
        }
        Page::Phones | Page::Pairing => {
            outline_round_rect(
                dc,
                RECT {
                    left: x - s(7),
                    right: x + s(7),
                    top: y - s(11),
                    bottom: y + s(11),
                },
                color,
                s(3),
            );
            fill_ellipse(
                dc,
                RECT {
                    left: x - s(1),
                    right: x + s(2),
                    top: y + s(7),
                    bottom: y + s(10),
                },
                color,
            );
        }
        Page::Settings => {}
    }
}
