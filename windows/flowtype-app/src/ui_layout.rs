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
        let content_left = 220.0;
        let content_right = (client_width - 18.0).max(content_left + 420.0);
        Self {
            content_left,
            content_right,
            title: Rect::from_xywh(content_left, 28.0, 300.0, 34.0),
            language_action: Rect::from_xywh(content_right - 34.0, 28.0, 34.0, 34.0),
        }
    }

    pub(super) fn action_before_language(self, width: f32) -> Rect {
        Rect::from_xywh(
            self.language_action.left - 12.0 - width,
            self.language_action.top,
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

impl StatusLayout {
    pub(super) fn new(client_width: f32) -> Self {
        let shell = ShellLayout::new(client_width);
        Self {
            shell,
            status_icon: Rect::from_xywh(shell.content_left, 44.0, 28.0, 28.0),
            summary: Rect::from_xywh(shell.content_left + 38.0, 38.0, 360.0, 40.0),
            separators: [140.0, 194.0, 248.0, 292.0],
            pair_action: shell.action_before_language(132.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SettingsLayout {
    pub shell: ShellLayout,
    pub save_action: Rect,
    pub name_edit: Rect,
    pub service_status_dot: Rect,
    pub repair_action: Rect,
    pub separators: [f32; 2],
    pub update_repository: Rect,
    pub update_history: Rect,
    pub update_action: Rect,
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
        let info_left = (shell.content_left + 250.0)
            .min(shell.content_right - 250.0)
            .max(shell.content_left + 230.0);
        let info_width = (shell.content_right - info_left).max(200.0);
        Self {
            shell,
            back_action: Rect::from_xywh(shell.content_left - 18.0, 32.0, 34.0, 34.0),
            title: Rect::from_xywh(shell.content_left + 26.0, 34.0, 300.0, 34.0),
            qr_origin: (shell.content_left, 130.0),
            info_left,
            open_instruction: Rect::from_xywh(info_left, 145.0, info_width, 44.0),
            scan_instruction: Rect::from_xywh(info_left, 201.0, info_width, 30.0),
            computer: Rect::from_xywh(info_left, 260.0, info_width, 28.0),
            waiting: Rect::from_xywh(info_left + 16.0, 298.0, info_width - 16.0, 26.0),
            note: Rect::from_xywh(info_left, 342.0, info_width, 60.0),
            waiting_dot: Rect::from_xywh(info_left, 307.0, 8.0, 8.0),
        }
    }
}

impl SettingsLayout {
    pub(super) fn new(client_width: f32) -> Self {
        let shell = ShellLayout::new(client_width);
        let save_action = shell.action_before_language(112.0);
        let update_action = Rect::from_xywh(shell.content_right - 138.0, 474.0, 138.0, 34.0);
        let update_history = Rect::from_xywh(update_action.left - 8.0 - 120.0, 476.0, 120.0, 32.0);
        let update_repository =
            Rect::from_xywh(update_history.left - 8.0 - 96.0, 476.0, 96.0, 32.0);
        Self {
            shell,
            save_action,
            name_edit: Rect::from_xywh(
                350.0,
                132.0,
                (shell.content_right - 350.0 - 102.0).clamp(220.0, 330.0),
                27.0,
            ),
            service_status_dot: Rect::from_xywh(shell.content_left + 6.0, 316.0, 12.0, 12.0),
            repair_action: Rect::from_xywh(shell.content_right - 112.0, 298.0, 112.0, 38.0),
            separators: [262.0, 382.0],
            update_repository,
            update_history,
            update_action,
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
        const CONTENT_LEFT: f32 = 220.0;
        const PAIR_WIDTH: f32 = 132.0;
        const ROW_TOP: f32 = 88.0;
        const ROW_HEIGHT: f32 = 84.0;

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
                let action = Rect::from_xywh(content_right - 94.0, top + 22.0, 94.0, 34.0);
                PhoneRowLayout {
                    bounds,
                    icon: Rect::from_xywh(CONTENT_LEFT + 2.0, top + 20.0, 34.0, 40.0),
                    name: Rect::from_xywh(
                        CONTENT_LEFT + 48.0,
                        top + 11.0,
                        (action.left - CONTENT_LEFT - 68.0).max(120.0),
                        28.0,
                    ),
                    status: Rect::from_xywh(
                        CONTENT_LEFT + 68.0,
                        top + 43.0,
                        (action.left - CONTENT_LEFT - 88.0).max(100.0),
                        24.0,
                    ),
                    status_dot: Rect::from_xywh(CONTENT_LEFT + 48.0, top + 50.0, 10.0, 10.0),
                    action,
                }
            })
            .collect();

        Self {
            title: shell.title,
            pair_action,
            language_action,
            empty_message: Rect::from_xywh(CONTENT_LEFT, 112.0, 420.0, 30.0),
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_actions_stay_right_aligned() {
        let layout = PhonesLayout::new(760.0, 1);
        assert_eq!(layout.language_action.right, 742.0);
        assert_eq!(layout.pair_action.right + 12.0, layout.language_action.left);
    }

    #[test]
    fn rows_share_the_available_width_without_overlapping_actions() {
        let layout = PhonesLayout::new(900.0, 2);
        assert_eq!(layout.rows.len(), 2);
        assert!(layout.rows[0].name.right < layout.rows[0].action.left);
        assert_eq!(layout.rows[0].bounds.bottom, layout.rows[1].bounds.top);
        assert_eq!(layout.rows[0].bounds.right, 882.0);
    }

    #[test]
    fn settings_actions_follow_the_shared_right_edge() {
        let layout = SettingsLayout::new(820.0);
        assert_eq!(layout.shell.language_action.right, 802.0);
        assert_eq!(
            layout.save_action.right + 12.0,
            layout.shell.language_action.left
        );
        assert_eq!(layout.repair_action.right, layout.shell.content_right);
        assert_eq!(layout.service_status_dot.right, 238.0);
        assert_eq!(layout.update_action.right, layout.shell.content_right);
        assert!(layout.update_repository.left >= layout.shell.content_left);
    }

    #[test]
    fn status_pair_action_shares_the_header_action_row() {
        let layout = StatusLayout::new(820.0);
        assert_eq!(layout.pair_action.top, layout.shell.language_action.top);
        assert_eq!(
            layout.pair_action.right + 12.0,
            layout.shell.language_action.left
        );
    }

    #[test]
    fn pairing_columns_remain_separate_at_minimum_width() {
        let layout = PairingLayout::new(680.0);
        assert!(layout.info_left >= layout.qr_origin.0 + 220.0);
        assert!(layout.info_left + 200.0 <= layout.shell.content_right + 20.0);
    }
}
