#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedState {
    pub sequence: i64,
    pub text: String,
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomingDisposition {
    Apply,
    Duplicate,
    Stale,
    Conflict,
    Finished,
}

impl AppliedState {
    pub fn new(sequence: i64, text: impl Into<String>) -> Self {
        Self {
            sequence,
            text: text.into(),
            finished: false,
        }
    }

    pub fn classify(&self, sequence: i64, text: &str) -> IncomingDisposition {
        if self.finished {
            return IncomingDisposition::Finished;
        }
        if sequence < self.sequence {
            return IncomingDisposition::Stale;
        }
        if sequence == self.sequence {
            return if text == self.text {
                IncomingDisposition::Duplicate
            } else {
                IncomingDisposition::Conflict
            };
        }
        IncomingDisposition::Apply
    }

    pub fn mark_applied(&mut self, sequence: i64, text: impl Into<String>, finished: bool) {
        self.sequence = sequence;
        self.text = text.into();
        self.finished = finished;
    }
}

#[cfg(test)]
mod tests {
    use super::{AppliedState, IncomingDisposition};

    #[test]
    fn classifies_sequence_updates() {
        let state = AppliedState::new(4, "当前文本");
        assert_eq!(state.classify(3, "旧文本"), IncomingDisposition::Stale);
        assert_eq!(
            state.classify(4, "当前文本"),
            IncomingDisposition::Duplicate
        );
        assert_eq!(state.classify(4, "冲突文本"), IncomingDisposition::Conflict);
        assert_eq!(state.classify(7, "最新文本"), IncomingDisposition::Apply);
    }

    #[test]
    fn rejects_changes_after_finish() {
        let mut state = AppliedState::new(1, "完成");
        state.mark_applied(2, "完成文本", true);
        assert_eq!(state.classify(3, "继续修改"), IncomingDisposition::Finished);
    }
}
