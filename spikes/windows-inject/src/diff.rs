use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub backspaces: usize,
    pub insert: String,
}

pub fn plan_transition(previous: &str, current: &str) -> Transition {
    let previous_graphemes: Vec<&str> = previous.graphemes(true).collect();
    let current_graphemes: Vec<&str> = current.graphemes(true).collect();

    let common_prefix = previous_graphemes
        .iter()
        .zip(&current_graphemes)
        .take_while(|(left, right)| left == right)
        .count();

    Transition {
        backspaces: previous_graphemes.len() - common_prefix,
        insert: current_graphemes[common_prefix..].concat(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Transition, plan_transition};

    #[test]
    fn appends_without_deleting() {
        assert_eq!(
            plan_transition("你好", "你好，Windows"),
            Transition {
                backspaces: 0,
                insert: "，Windows".to_owned(),
            }
        );
    }

    #[test]
    fn replaces_an_arbitrary_tail() {
        assert_eq!(
            plan_transition("豆包正在识别语音", "豆包正在识别文本"),
            Transition {
                backspaces: 2,
                insert: "文本".to_owned(),
            }
        );
    }

    #[test]
    fn deletes_a_whole_emoji_grapheme() {
        assert_eq!(
            plan_transition("家庭👨‍👩‍👧‍👦", "家庭"),
            Transition {
                backspaces: 1,
                insert: String::new(),
            }
        );
    }

    #[test]
    fn keeps_combining_marks_on_a_grapheme_boundary() {
        assert_eq!(
            plan_transition("Cafe\u{301}", "Cafe\u{301} 好"),
            Transition {
                backspaces: 0,
                insert: " 好".to_owned(),
            }
        );
    }

    #[test]
    fn can_replace_everything() {
        assert_eq!(
            plan_transition("旧内容", "new"),
            Transition {
                backspaces: 3,
                insert: "new".to_owned(),
            }
        );
    }
}
