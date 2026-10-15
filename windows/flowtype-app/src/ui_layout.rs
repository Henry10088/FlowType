use super::FileTab;

pub(super) const MIN_CLIENT_WIDTH: i32 = 780;
// The update area ends at 577 DIP; keep it visible with a bottom margin.
pub(super) const MIN_CLIENT_HEIGHT: i32 = 600;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub(super) const fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    pub(super) fn width(self) -> f32 {
        self.right - self.left
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ShellLayout {
    pub content_left: f32,
    pub content_right: f32,
    pub title: Rect,
    pub language_action: Rect,
}

impl ShellLayout {
    pub(super) fn new(client_width: f32) -> Self {
        let content_left = 244.0;
        let content_right = (client_width - 42.0).max(content_left + 420.0);
        Self {
            content_left,
            content_right,
            title: Rect::from_xywh(content_left, 75.0, 300.0, 36.0),
            language_action: Rect::from_xywh(155.0, 540.0, 34.0, 34.0),
        }
    }

    pub(super) fn action_before_language(self, width: f32) -> Rect {
        Rect::from_xywh(
            self.content_right - width,
            self.title.top,
            width,
            self.language_action.bottom - self.language_action.top,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct StatusLayout {
    pub shell: ShellLayout,
    pub status_icon: Rect,
    pub summary: Rect,
    pub separators: [f32; 4],
    pub pair_action: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FilesLayout {
    pub destination: Rect,
    pub status: Rect,
    pub failures: Rect,
    pub drop_hint: Rect,
    pub security_note: Rect,
    pub receive_root: Rect,
    pub receive_action: Rect,
    pub cancel_action: Rect,
}

impl FilesLayout {
    pub(super) fn new(client_width: f32, tab: FileTab) -> Self {
        let right = ShellLayout::new(client_width).content_right;
        let text_width = right - 244.0 - 16.0;
        let received = tab == FileTab::Received;
        Self {
            destination: Rect::from_xywh(320.0, 199.0, text_width - 76.0, 26.0),
            status: Rect::from_xywh(
                260.0,
                if received { 355.0 } else { 434.0 },
                text_width - 16.0,
                32.0,
            ),
            failures: Rect::from_xywh(
                260.0,
                if received { 393.0 } else { 470.0 },
                text_width - 16.0,
                42.0,
            ),
            drop_hint: Rect::from_xywh(370.0, 305.0, text_width - 150.0, 30.0),
            security_note: Rect::from_xywh(
                260.0,
                if received { 492.0 } else { 522.0 },
                text_width - 16.0,
                28.0,
            ),
            receive_root: Rect::from_xywh(350.0, 269.0, right - 482.0, 28.0),
            receive_action: Rect::from_xywh(right - 120.0, 263.0, 108.0, 34.0),
            cancel_action: if received {
                Rect::from_xywh(260.0, 444.0, 120.0, 34.0)
            } else {
                Rect::from_xywh(370.0, 354.0, 120.0, 34.0)
            },
        }
    }
}

impl StatusLayout {
    pub(super) fn new(client_width: f32) -> Self {
        let shell = ShellLayout::new(client_width);
        Self {
            shell,
            status_icon: Rect::from_xywh(shell.content_left + 22.0, 157.0, 53.0, 53.0),
            summary: Rect::from_xywh(
                shell.content_left + 93.0,
                154.0,
                (shell.content_right - shell.content_left - 115.0).min(440.0),
                35.0,
            ),
            separators: [255.0, 310.0, 365.0, 420.0],
            pair_action: Rect::from_xywh(shell.content_left, 493.0, 136.0, 35.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SettingsLayout {
    pub shell: ShellLayout,
    pub save_action: Rect,
    pub name_edit: Rect,
    pub name_frame: Rect,
    pub service_status_dot: Rect,
    pub repair_action: Rect,
    pub separators: [f32; 2],
    pub update_repository: Rect,
    pub update_history: Rect,
    pub update_action: Rect,
    pub update_status: Rect,
    pub update_progress: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PairingLayout {
    pub shell: ShellLayout,
    pub back_action: Rect,
    pub title: Rect,
    pub qr_origin: (f32, f32),
    pub info_left: f32,
    pub open_instruction: Rect,
    pub scan_instruction: Rect,
    pub computer: Rect,
    pub waiting: Rect,
    pub note: Rect,
    pub waiting_dot: Rect,
}

impl PairingLayout {
    pub(super) fn new(client_width: f32) -> Self {
        let shell = ShellLayout::new(client_width);
        let info_left = shell.content_left + 288.0;
        let info_width = (shell.content_right - info_left - 30.0).max(160.0);
        Self {
            shell,
            back_action: Rect::from_xywh(shell.content_right - 110.0, 78.0, 110.0, 32.0),
            title: shell.title,
            qr_origin: (shell.content_left + 58.0, 225.0),
            info_left,
            open_instruction: Rect::from_xywh(info_left + 40.0, 231.0, info_width - 40.0, 36.0),
            scan_instruction: Rect::from_xywh(info_left + 40.0, 316.0, info_width - 40.0, 36.0),
            computer: Rect::from_xywh(info_left + 40.0, 265.0, info_width - 40.0, 28.0),
            waiting: Rect::from_xywh(info_left + 40.0, 351.0, info_width - 40.0, 26.0),
            note: Rect::from_xywh(shell.content_left, 484.0, 480.0, 36.0),
            waiting_dot: Rect::from_xywh(info_left + 40.0, 363.0, 7.0, 7.0),
        }
    }
}

impl SettingsLayout {
    pub(super) fn new(client_width: f32) -> Self {
        let shell = ShellLayout::new(client_width);
        let save_action = Rect::from_xywh(shell.content_right - 85.0, 157.0, 85.0, 35.0);
        let update_action = Rect::from_xywh(shell.content_right - 108.0, 506.0, 108.0, 35.0);
        let update_history = Rect::from_xywh(update_action.left - 8.0 - 112.0, 507.0, 112.0, 34.0);
        let update_repository =
            Rect::from_xywh(update_history.left - 8.0 - 100.0, 507.0, 100.0, 34.0);
        let name_edit = Rect::from_xywh(
            385.0,
            160.0,
            (shell.content_right - 385.0 - 100.0).clamp(180.0, 235.0),
            30.0,
        );
        Self {
            shell,
            save_action,
            name_edit,
            name_frame: Rect::from_xywh(
                name_edit.left - 8.0,
                name_edit.top - 3.0,
                name_edit.width() + 16.0,
                36.0,
            ),
            service_status_dot: Rect::from_xywh(385.0, 369.0, 8.0, 8.0),
            repair_action: Rect::from_xywh(shell.content_right - 118.0, 355.0, 118.0, 35.0),
            separators: [310.0, 420.0],
            update_repository,
            update_history,
            update_action,
            update_status: Rect::from_xywh(385.0, 544.0, shell.content_right - 385.0, 24.0),
            update_progress: Rect::from_xywh(385.0, 571.0, shell.content_right - 385.0, 6.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PhoneRowLayout {
    pub bounds: Rect,
    pub icon: Rect,
    pub name: Rect,
    pub status: Rect,
    pub status_dot: Rect,
    pub action: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PhonesLayout {
    pub title: Rect,
    pub pair_action: Rect,
    pub language_action: Rect,
    pub empty_message: Rect,
    pub rows: Vec<PhoneRowLayout>,
}

impl PhonesLayout {
    pub(super) fn new(client_width: f32, row_count: usize) -> Self {
        const CONTENT_LEFT: f32 = 244.0;
        const PAIR_WIDTH: f32 = 132.0;
        const ROW_TOP: f32 = 169.0;
        const ROW_HEIGHT: f32 = 94.0;

        let shell = ShellLayout::new(client_width);
        let content_right = shell.content_right;
        let language_action = shell.language_action;
        let pair_action = shell.action_before_language(PAIR_WIDTH);
        let rows = (0..row_count)
            .map(|index| {
                let top = ROW_TOP + index as f32 * ROW_HEIGHT;
                let bounds = Rect {
                    left: CONTENT_LEFT,
                    top,
                    right: content_right,
                    bottom: top + ROW_HEIGHT,
                };
                let action = Rect::from_xywh(content_right - 100.0, top + 29.0, 100.0, 35.0);
                PhoneRowLayout {
                    bounds,
                    icon: Rect::from_xywh(CONTENT_LEFT + 10.0, top + 25.0, 43.0, 43.0),
                    name: Rect::from_xywh(
                        CONTENT_LEFT + 70.0,
                        top + 20.0,
                        (action.left - CONTENT_LEFT - 90.0).max(120.0),
                        28.0,
                    ),
                    status: Rect::from_xywh(
                        CONTENT_LEFT + 84.0,
                        top + 51.0,
                        (action.left - CONTENT_LEFT - 100.0).max(100.0),
                        24.0,
                    ),
                    status_dot: Rect::from_xywh(CONTENT_LEFT + 70.0, top + 58.0, 7.0, 7.0),
                    action,
                }
            })
            .collect();

        Self {
            title: shell.title,
            pair_action,
            language_action,
            empty_message: Rect::from_xywh(CONTENT_LEFT + 20.0, 202.0, 420.0, 30.0),
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_receive_path_does_not_overlap_change_button() {
        for width in [680.0, 760.0, 1200.0] {
            let layout = FilesLayout::new(width, FileTab::Received);
            assert!(layout.receive_root.right < layout.receive_action.left);
            assert!(layout.receive_action.right <= width);
        }
    }

    #[test]
    fn phone_pair_action_stays_right_aligned() {
        let layout = PhonesLayout::new(760.0, 1);
        assert_eq!(layout.pair_action.right, layout.rows[0].bounds.right);
        assert!(layout.language_action.right < layout.title.left);
    }

    #[test]
    fn rows_share_the_available_width_without_overlapping_actions() {
        let layout = PhonesLayout::new(900.0, 2);
        assert_eq!(layout.rows.len(), 2);
        assert!(layout.rows[0].name.right < layout.rows[0].action.left);
        assert_eq!(layout.rows[0].bounds.bottom, layout.rows[1].bounds.top);
        assert_eq!(layout.rows[0].bounds.right, 858.0);
    }

    #[test]
    fn settings_actions_follow_the_shared_right_edge() {
        let layout = SettingsLayout::new(820.0);
        assert_eq!(layout.shell.content_right, 778.0);
        assert_eq!(layout.save_action.right, layout.shell.content_right);
        assert_eq!(layout.repair_action.right, layout.shell.content_right);
        assert_eq!(layout.service_status_dot.right, 393.0);
        assert_eq!(layout.update_action.right, layout.shell.content_right);
        assert!(layout.update_repository.left >= layout.shell.content_left);
    }

    #[test]
    fn status_pair_action_sits_below_details() {
        let layout = StatusLayout::new(820.0);
        assert!(layout.pair_action.top > layout.separators[3]);
        assert_eq!(layout.pair_action.left, layout.shell.content_left);
    }

    #[test]
    fn pairing_columns_remain_separate_at_minimum_width() {
        let layout = PairingLayout::new(780.0);
        assert!(layout.info_left >= layout.qr_origin.0 + 168.0);
        assert!(layout.info_left + 160.0 <= layout.shell.content_right);
    }

    #[test]
    fn settings_update_area_fits_the_minimum_window_at_multiple_dpis() {
        for width in [MIN_CLIENT_WIDTH as f32, 980.0, 1200.0] {
            let layout = SettingsLayout::new(width);
            for dpi in [96.0, 120.0, 144.0, 192.0] {
                let scale = dpi / 96.0;
                for rect in [
                    layout.update_repository,
                    layout.update_history,
                    layout.update_action,
                    layout.update_status,
                    layout.update_progress,
                ] {
                    assert!(rect.width() > 0.0);
                    assert!(rect.left >= layout.shell.content_left);
                    assert!(
                        (rect.right * scale).round()
                            <= (layout.shell.content_right * scale).round()
                    );
                    assert!(
                        (rect.bottom * scale).round()
                            <= ((MIN_CLIENT_HEIGHT - 16) as f32 * scale).round()
                    );
                }
            }
            assert!(layout.update_action.bottom < layout.update_status.top);
            assert!(layout.update_status.bottom < layout.update_progress.top);
        }
    }

    #[test]
    fn receive_cancel_action_stays_visible_and_separate_from_status_text() {
        let layout = FilesLayout::new(MIN_CLIENT_WIDTH as f32, FileTab::Received);
        assert!(layout.status.bottom < layout.failures.top);
        assert!(layout.failures.bottom < layout.cancel_action.top);
        assert!(layout.cancel_action.bottom < layout.security_note.top);
        assert!(layout.cancel_action.right <= MIN_CLIENT_WIDTH as f32);
        assert!(layout.security_note.bottom < MIN_CLIENT_HEIGHT as f32);
    }
}
