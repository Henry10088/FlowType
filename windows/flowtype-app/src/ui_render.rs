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
                    self.navigation_font,
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
                fill(
                    item.hDC,
                    &rect,
                    if pressed {
                        COLOR_TEAL_PALE
                    } else {
                        COLOR_WHITE
                    },
                );
                outline_round_rect(item.hDC, rect, COLOR_LINE, self.scale(4));
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
            outline_round_rect(item.hDC, focus, COLOR_TEAL, self.scale(4));
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
        let dc = unsafe { BeginPaint(self.hwnd, &mut paint) };
        let mut client: RECT = unsafe { zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut client) };
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32;
        let logical_height = client.bottom as f32 * 96.0 / dpi;
        if self.page == Page::Phones {
            let snapshot = self.state.snapshot();
            let layout = self.phones_layout();
            let rows = snapshot
                .phones
                .iter()
                .map(|(_, phone)| {
                    let connected = snapshot.status.connected_phone.as_deref()
                        == Some(phone.phone_name.as_str());
                    PhonePaintRow { connected }
                })
                .collect::<Vec<_>>();
            if let Some(painter) = self.direct2d.as_ref() {
                let _ = painter.paint_phones(logical_height, &layout, &rows);
                unsafe { EndPaint(self.hwnd, &paint) };
                return;
            }
        }
        if let Some(painter) = self.direct2d.as_ref() {
            let painted = match self.page {
                Page::Status => Some(painter.paint_status(
                    logical_height,
                    &StatusLayout::new(self.logical_client_width()),
                )),
                Page::Settings if self.save_notice.is_none() => Some(painter.paint_settings(
                    logical_height,
                    &SettingsLayout::new(self.logical_client_width()),
                    self.state.snapshot().injector_ready,
                )),
                Page::Pairing => Some(
                    painter.paint_pairing(
                        logical_height,
                        &PairingLayout::new(self.logical_client_width()),
                        self.qr
                            .as_ref()
                            .map(|(width, modules)| (*width, modules.as_slice())),
                    ),
                ),
                _ => None,
            };
            if painted.is_some_and(|result| result.is_ok()) {
                unsafe { EndPaint(self.hwnd, &paint) };
                return;
            }
        }
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
            Page::Phones => {}
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
