use serde::{Deserialize, Serialize};

use crate::{MAX_MESSAGE_BYTES, PROTOCOL_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Start(Snapshot),
    Update(Snapshot),
    Finish(Snapshot),
    Resume(Resume),
    Cancel(Cancel),
    Probe(Probe),
    HealthCheck(HealthCheck),
    SwitchAck(SwitchAck),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub protocol_version: u16,
    pub phone_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub full_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub protocol_version: u16,
    pub phone_id: String,
    pub session_id: String,
    pub last_ack_sequence: i64,
    pub sequence: i64,
    pub full_text: String,
    pub session_state: ClientSessionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancel {
    pub protocol_version: u16,
    pub phone_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub protocol_version: u16,
    pub phone_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCheck {
    pub protocol_version: u16,
    pub phone_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchAck {
    pub protocol_version: u16,
    pub request_id: String,
    pub pc_id: String,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientSessionState {
    Active,
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ack(Ack),
    Target(Target),
    SwitchComputer(SwitchComputer),
    ProbeResult(ProbeResult),
    HealthAck(HealthAck),
    Error(ProtocolError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchComputer {
    pub protocol_version: u16,
    pub pc_id: String,
    pub pc_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthAck {
    pub protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub protocol_version: u16,
    pub target_state: ProbeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    Ready,
    Unsupported,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    pub protocol_version: u16,
    pub session_id: String,
    pub applied_sequence: i64,
    pub session_state: ServerSessionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerSessionState {
    Active,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub protocol_version: u16,
    pub session_id: String,
    pub target_state: TargetState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetState {
    Active,
    NotForeground,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub protocol_version: u16,
    pub code: ErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AuthFailed,
    SessionBusy,
    SessionUnknown,
    SessionFinished,
    SequenceConflict,
    TextTooLarge,
    TargetUnavailable,
    TargetInvalid,
    TargetModified,
    InjectorUnavailable,
    InjectionUnknown,
    InvalidMessage,
    UnsupportedProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    UnsupportedProtocol,
    MissingIdentifier,
    InvalidSequence,
    MessageTooLarge,
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocol);
        }
        if self.phone_id.is_empty() || self.session_id.is_empty() {
            return Err(ValidationError::MissingIdentifier);
        }
        if self.sequence <= 0 {
            return Err(ValidationError::InvalidSequence);
        }
        if self.full_text.len() > MAX_MESSAGE_BYTES {
            return Err(ValidationError::MessageTooLarge);
        }
        Ok(())
    }
}

impl Resume {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocol);
        }
        if self.phone_id.is_empty() || self.session_id.is_empty() {
            return Err(ValidationError::MissingIdentifier);
        }
        if self.sequence <= 0
            || self.last_ack_sequence < 0
            || self.last_ack_sequence > self.sequence
        {
            return Err(ValidationError::InvalidSequence);
        }
        if self.full_text.len() > MAX_MESSAGE_BYTES {
            return Err(ValidationError::MessageTooLarge);
        }
        Ok(())
    }
}

impl Cancel {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocol);
        }
        if self.phone_id.is_empty() || self.session_id.is_empty() {
            return Err(ValidationError::MissingIdentifier);
        }
        Ok(())
    }
}

impl Probe {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocol);
        }
        if self.phone_id.is_empty() {
            return Err(ValidationError::MissingIdentifier);
        }
        Ok(())
    }
}

impl HealthCheck {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocol);
        }
        if self.phone_id.is_empty() {
            return Err(ValidationError::MissingIdentifier);
        }
        Ok(())
    }
}

impl SwitchAck {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocol);
        }
        if self.request_id.is_empty() || self.pc_id.is_empty() {
            return Err(ValidationError::MissingIdentifier);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClientMessage, ErrorCode, HealthAck, HealthCheck, ProtocolError, ServerMessage, Snapshot,
        SwitchAck, SwitchComputer, ValidationError,
    };

    #[test]
    fn parses_language_neutral_contract_fixtures() {
        let json = include_str!("../../../protocol/v1/valid-messages.json");
        let values: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();

        for value in values.iter().take(3) {
            let message: ClientMessage = serde_json::from_value(value.clone()).unwrap();
            let snapshot = match message {
                ClientMessage::Start(value)
                | ClientMessage::Update(value)
                | ClientMessage::Finish(value) => value,
                ClientMessage::Resume(_)
                | ClientMessage::Cancel(_)
                | ClientMessage::Probe(_)
                | ClientMessage::HealthCheck(_)
                | ClientMessage::SwitchAck(_) => {
                    unreachable!()
                }
            };
            snapshot.validate().unwrap();
        }

        let ack: ServerMessage = serde_json::from_value(values[3].clone()).unwrap();
        assert!(matches!(ack, ServerMessage::Ack(_)));
    }

    #[test]
    fn rejects_non_positive_sequences() {
        let value = Snapshot {
            protocol_version: 1,
            phone_id: "phone".into(),
            session_id: "session".into(),
            sequence: 0,
            full_text: String::new(),
        };
        assert_eq!(value.validate(), Err(ValidationError::InvalidSequence));
    }

    #[test]
    fn serializes_input_service_failures_without_closing_the_protocol() {
        let message = ServerMessage::Error(ProtocolError {
            protocol_version: 1,
            code: ErrorCode::InjectorUnavailable,
            session_id: Some("session".into()),
        });
        let value = serde_json::to_value(message).unwrap();

        assert_eq!(value["type"], "error");
        assert_eq!(value["code"], "INJECTOR_UNAVAILABLE");
        assert_eq!(value["session_id"], "session");
    }

    #[test]
    fn parses_session_cancellation() {
        let message: ClientMessage = serde_json::from_str(
            r#"{"protocol_version":1,"type":"cancel","phone_id":"phone","session_id":"session"}"#,
        )
        .unwrap();

        let ClientMessage::Cancel(cancel) = message else {
            panic!("expected cancel message");
        };
        cancel.validate().unwrap();
    }

    #[test]
    fn validates_target_probe_and_result() {
        let probe = ClientMessage::Probe(super::Probe {
            protocol_version: 1,
            phone_id: "phone".into(),
        });
        let value = serde_json::to_value(&probe).unwrap();
        assert_eq!(value["type"], "probe");
        assert!(matches!(probe, ClientMessage::Probe(value) if value.validate().is_ok()));

        let result = ServerMessage::ProbeResult(super::ProbeResult {
            protocol_version: 1,
            target_state: super::ProbeState::Ready,
            target_name: Some("VS Code".into()),
            activity_age_ms: Some(42),
        });
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["type"], "probe_result");
        assert_eq!(value["activity_age_ms"], 42);
    }

    #[test]
    fn serializes_switch_to_current_computer() {
        let value = serde_json::to_value(ServerMessage::SwitchComputer(SwitchComputer {
            protocol_version: 1,
            pc_id: "pc".into(),
            pc_name: "办公室电脑".into(),
            request_id: "request-1".into(),
        }))
        .unwrap();
        assert_eq!(value["type"], "switch_computer");
        assert_eq!(value["pc_id"], "pc");
        assert_eq!(value["pc_name"], "办公室电脑");
        assert_eq!(value["request_id"], "request-1");
    }

    #[test]
    fn validates_health_check_and_switch_ack() {
        let health = ClientMessage::HealthCheck(HealthCheck {
            protocol_version: 1,
            phone_id: "phone".into(),
        });
        assert!(matches!(health, ClientMessage::HealthCheck(value) if value.validate().is_ok()));
        let value = serde_json::to_value(ServerMessage::HealthAck(HealthAck {
            protocol_version: 1,
        }))
        .unwrap();
        assert_eq!(value["type"], "health_ack");

        let ack = ClientMessage::SwitchAck(SwitchAck {
            protocol_version: 1,
            request_id: "request-1".into(),
            pc_id: "pc".into(),
            accepted: true,
        });
        let value = serde_json::to_value(&ack).unwrap();
        assert_eq!(value["type"], "switch_ack");
        assert!(matches!(ack, ClientMessage::SwitchAck(value) if value.validate().is_ok()));
    }
}
