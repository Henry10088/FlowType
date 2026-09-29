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
                if focused {
                    outline_round_rect(
                        item.hDC,
                        selection,
                        COLOR_TEAL,
                        self.scale(CORNER_RADIUS * 2),
                    );
                }
                let icon = match target {
                    Page::Status => "\u{e7f4}",
                    Page::Files => "\u{e8b7}",
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
                    self.navigation_font,
                    if disabled { COLOR_MUTED } else { COLOR_TEXT },
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
                if visual.hovered {
                    fill_round_rect(item.hDC, rect, COLOR_HOVER, self.scale(CORNER_RADIUS));
                }
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
                    self.scale(6),
                );
                if checked {
                    draw_checkmark(item.hDC, box_rect, COLOR_WHITE);
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
        let panels = page_panels(self.page, self.logical_client_width(), self.phone_ids.len());
        {
            let result = self.direct2d.as_ref().map(|painter| match self.page {
                Page::Status => painter.paint_status(
                    logical_height,
                    &StatusLayout::new(self.logical_client_width()),
                    &panels,
                    self.save_notice.as_ref(),
                ),
                Page::Files => {
                    painter.paint_files(logical_height, &panels, self.save_notice.as_ref())
                }
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
        for panel in panels {
            draw_panel(
                dc,
                device_rect(panel, dpi as u32),
                self.scale(CORNER_RADIUS),
            );
        }

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
                let layout = self.phones_layout();
                for ((_, phone), row) in snapshot.phones.iter().zip(layout.rows.iter()) {
                    let icon = row.icon;
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
            Page::Files => {}
            Page::Settings => {
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
