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
    fn appends_text() {
        assert_eq!(
            plan_transition("你好", "你好，Windows"),
            Transition {
                backspaces: 0,
                insert: "，Windows".to_owned(),
            }
        );
    }

    #[test]
    fn rewrites_an_arbitrary_tail() {
        assert_eq!(
            plan_transition("正在输入旧内容", "正在输入新文本"),
            Transition {
                backspaces: 3,
                insert: "新文本".to_owned(),
            }
        );
    }

    #[test]
    fn deletes_one_emoji_grapheme() {
        assert_eq!(
            plan_transition("家庭👨‍👩‍👧‍👦", "家庭"),
            Transition {
                backspaces: 1,
                insert: String::new(),
            }
        );
    }

    #[test]
    fn preserves_combining_marks() {
        assert_eq!(
            plan_transition("Cafe\u{301}", "Cafe\u{301} 好"),
            Transition {
                backspaces: 0,
                insert: " 好".to_owned(),
            }
        );
    }
}
