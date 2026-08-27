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
        const CONTENT_RIGHT_PADDING: f32 = 18.0;
        const HEADER_TOP: f32 = 28.0;
        const HEADER_HEIGHT: f32 = 34.0;
        const LANGUAGE_WIDTH: f32 = 34.0;
        const PAIR_WIDTH: f32 = 132.0;
        const HEADER_GAP: f32 = 12.0;
        const ROW_TOP: f32 = 88.0;
        const ROW_HEIGHT: f32 = 84.0;

        let content_right = (client_width - CONTENT_RIGHT_PADDING).max(CONTENT_LEFT + 420.0);
        let language_action = Rect::from_xywh(
            content_right - LANGUAGE_WIDTH,
            HEADER_TOP,
            LANGUAGE_WIDTH,
            HEADER_HEIGHT,
        );
        let pair_action = Rect::from_xywh(
            language_action.left - HEADER_GAP - PAIR_WIDTH,
            HEADER_TOP,
            PAIR_WIDTH,
            HEADER_HEIGHT,
        );
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
            title: Rect::from_xywh(CONTENT_LEFT, 28.0, 280.0, HEADER_HEIGHT),
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
}
