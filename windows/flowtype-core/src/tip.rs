use serde::{Deserialize, Serialize};

pub const TIP_PIPE_NAME: &str = r"\\.\pipe\flowtype-tip-v4";
pub const CLSID_FLOWTYPE_TIP_VALUE: u128 = 0x9a50b266_9e86_4ff4_871b_8d47ad8c658b;
pub const GUID_FLOWTYPE_PROFILE_VALUE: u128 = 0x567ab276_3af1_4874_8e2c_d47c31d5e46e;
pub const FLOWTYPE_LANG_ID: u16 = 0x0804;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipHello {
    pub ipc_version: u16,
    pub component_version: String,
    pub process_id: u32,
    pub thread_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TipCommand {
    Begin {
        session_id: String,
        sequence: i64,
        full_text: String,
        attach_existing: bool,
    },
    Update {
        session_id: String,
        sequence: i64,
        full_text: String,
    },
    Finish {
        session_id: String,
        sequence: i64,
    },
    Cancel {
        session_id: String,
    },
    Query {
        session_id: String,
    },
    Ping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TipResponse {
    Ready,
    Begun { session_id: String },
    Applied { session_id: String, sequence: i64 },
    Finished { session_id: String, sequence: i64 },
    Cancelled { session_id: String },
    NoFocus,
    SessionMismatch,
    SequenceConflict,
    CompositionTerminated,
    SessionActive { session_id: String, sequence: i64 },
    RebindRejected,
    EditRejected,
}

#[derive(Debug, Default)]
pub struct TipSessionModel {
    session_id: Option<String>,
    sequence: i64,
    text: String,
    composition_ended: bool,
    target_modified: bool,
}

impl TipSessionModel {
    pub fn begin(
        &mut self,
        session_id: &str,
        sequence: i64,
        full_text: &str,
    ) -> Result<(), TipResponse> {
        if self.session_id.as_deref() == Some(session_id) {
            return if self.target_modified {
                Err(TipResponse::CompositionTerminated)
            } else {
                Ok(())
            };
        }
        if self.session_id.is_some() {
            return Err(TipResponse::SessionMismatch);
        }
        self.session_id = Some(session_id.to_owned());
        self.sequence = sequence;
        self.text.clear();
        self.text.push_str(full_text);
        self.composition_ended = false;
        self.target_modified = false;
        Ok(())
    }

    pub fn update(
        &mut self,
        session_id: &str,
        sequence: i64,
        full_text: &str,
    ) -> Result<bool, TipResponse> {
        if self.session_id.as_deref() != Some(session_id) {
            return Err(TipResponse::SessionMismatch);
        }
        if self.target_modified {
            return Err(TipResponse::CompositionTerminated);
        }
        if sequence < self.sequence {
            return Ok(false);
        }
        if sequence == self.sequence {
            return if self.text == full_text {
                Ok(false)
            } else {
                Err(TipResponse::SequenceConflict)
            };
        }
        self.sequence = sequence;
        self.text.clear();
        self.text.push_str(full_text);
        Ok(true)
    }

    pub fn terminate(&mut self) {
        self.target_modified = true;
    }

    pub fn end_composition(&mut self) {
        self.composition_ended = true;
    }

    pub fn finish(&mut self, session_id: &str, sequence: i64) -> Result<(), TipResponse> {
        if self.session_id.as_deref() != Some(session_id) || self.sequence != sequence {
            return Err(TipResponse::SessionMismatch);
        }
        if self.target_modified {
            return Err(TipResponse::CompositionTerminated);
        }
        self.clear();
        Ok(())
    }

    pub fn cancel(&mut self, session_id: &str) -> Result<(), TipResponse> {
        if self.session_id.as_deref() != Some(session_id) {
            return Err(TipResponse::SessionMismatch);
        }
        self.clear();
        Ok(())
    }

    fn clear(&mut self) {
        self.session_id = None;
        self.sequence = 0;
        self.text.clear();
        self.composition_ended = false;
        self.target_modified = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{TipHello, TipResponse, TipSessionModel};

    #[test]
    fn tip_handshake_carries_its_independent_protocol_version() {
        let value = serde_json::to_value(TipHello {
            ipc_version: crate::TIP_IPC_VERSION,
            component_version: env!("CARGO_PKG_VERSION").to_owned(),
            process_id: 42,
            thread_id: 7,
        })
        .unwrap();

        assert_eq!(value["ipc_version"], crate::TIP_IPC_VERSION);
        assert_eq!(value["component_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["process_id"], 42);
        assert_eq!(value["thread_id"], 7);
    }

    #[test]
    fn full_snapshots_replace_the_active_composition() {
        let mut model = TipSessionModel::default();
        model.begin("voice", 1, "现在是八点").unwrap();
        assert_eq!(model.update("voice", 2, "现在是八点一刻"), Ok(true));
        assert_eq!(model.update("voice", 3, "现在八点十四分"), Ok(true));
        model.finish("voice", 3).unwrap();
    }

    #[test]
    fn duplicate_snapshots_are_idempotent_but_conflicts_are_rejected() {
        let mut model = TipSessionModel::default();
        model.begin("voice", 1, "你好").unwrap();
        assert_eq!(model.update("voice", 1, "你好"), Ok(false));
        assert_eq!(
            model.update("voice", 1, "不同内容"),
            Err(TipResponse::SequenceConflict)
        );
    }

    #[test]
    fn repeated_begin_for_the_same_session_preserves_the_composition() {
        let mut model = TipSessionModel::default();
        model.begin("voice", 1, "已有正文").unwrap();

        model.begin("voice", 1, "已有正文").unwrap();

        assert_eq!(model.update("voice", 2, "修正正文"), Ok(true));
    }

    #[test]
    fn a_trailing_newline_remains_ordinary_composition_text() {
        let mut model = TipSessionModel::default();
        model.begin("voice", 1, "第一行\n").unwrap();
        model.end_composition();
        assert_eq!(model.update("voice", 2, "第一行\n第二行"), Ok(true));
        model.finish("voice", 2).unwrap();
    }

    #[test]
    fn a_new_session_cannot_replace_an_unfinished_range() {
        let mut model = TipSessionModel::default();
        model.begin("voice-1", 1, "已有正文").unwrap();
        model.end_composition();

        assert_eq!(
            model.begin("voice-2", 1, "新正文"),
            Err(TipResponse::SessionMismatch)
        );
        assert_eq!(model.update("voice-1", 2, "修正正文"), Ok(true));
    }

    #[test]
    fn target_termination_is_never_acknowledged_as_applied() {
        let mut model = TipSessionModel::default();
        model.begin("voice", 1, "输入中").unwrap();
        model.terminate();
        assert_eq!(
            model.update("voice", 2, "修正后"),
            Err(TipResponse::CompositionTerminated)
        );
    }
}
