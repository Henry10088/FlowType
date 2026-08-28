use serde::{Deserialize, Serialize};

pub const TIP_PIPE_NAME: &str = r"\\.\pipe\flowtype-tip-v2";
pub const CLSID_FLOWTYPE_TIP_VALUE: u128 = 0x9a50b266_9e86_4ff4_871b_8d47ad8c658b;
pub const GUID_FLOWTYPE_PROFILE_VALUE: u128 = 0x567ab276_3af1_4874_8e2c_d47c31d5e46e;
pub const FLOWTYPE_LANG_ID: u16 = 0x0804;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipHello {
    pub ipc_version: u16,
    pub process_id: u32,
    pub thread_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TipCommand {
    Begin {
        session_id: String,
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
    EditRejected,
}

#[derive(Debug, Default)]
pub struct TipSessionModel {
    session_id: Option<String>,
    sequence: i64,
    text: String,
    terminated: bool,
}

impl TipSessionModel {
    pub fn begin(&mut self, session_id: &str) -> Result<(), TipResponse> {
        if self.session_id.is_some() {
            return Err(TipResponse::SessionMismatch);
        }
        self.session_id = Some(session_id.to_owned());
        self.sequence = 0;
        self.text.clear();
        self.terminated = false;
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
        if self.terminated {
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
        self.terminated = true;
    }

    pub fn finish(&mut self, session_id: &str, sequence: i64) -> Result<(), TipResponse> {
        if self.session_id.as_deref() != Some(session_id) || self.sequence != sequence {
            return Err(TipResponse::SessionMismatch);
        }
        if self.terminated {
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
        self.terminated = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{TipHello, TipResponse, TipSessionModel};

    #[test]
    fn tip_handshake_carries_its_independent_protocol_version() {
        let value = serde_json::to_value(TipHello {
            ipc_version: crate::TIP_IPC_VERSION,
            process_id: 42,
            thread_id: 7,
        })
        .unwrap();

        assert_eq!(value["ipc_version"], crate::TIP_IPC_VERSION);
        assert_eq!(value["process_id"], 42);
        assert_eq!(value["thread_id"], 7);
    }

    #[test]
    fn full_snapshots_replace_the_active_composition() {
        let mut model = TipSessionModel::default();
        model.begin("voice").unwrap();
        assert_eq!(model.update("voice", 1, "现在是八点"), Ok(true));
        assert_eq!(model.update("voice", 2, "现在是八点一刻"), Ok(true));
        assert_eq!(model.update("voice", 3, "现在八点十四分"), Ok(true));
        model.finish("voice", 3).unwrap();
    }

    #[test]
    fn duplicate_snapshots_are_idempotent_but_conflicts_are_rejected() {
        let mut model = TipSessionModel::default();
        model.begin("voice").unwrap();
        assert_eq!(model.update("voice", 1, "你好"), Ok(true));
        assert_eq!(model.update("voice", 1, "你好"), Ok(false));
        assert_eq!(
            model.update("voice", 1, "不同内容"),
            Err(TipResponse::SequenceConflict)
        );
    }

    #[test]
    fn target_termination_is_never_acknowledged_as_applied() {
        let mut model = TipSessionModel::default();
        model.begin("voice").unwrap();
        model.update("voice", 1, "输入中").unwrap();
        model.terminate();
        assert_eq!(
            model.update("voice", 2, "修正后"),
            Err(TipResponse::CompositionTerminated)
        );
    }
}
