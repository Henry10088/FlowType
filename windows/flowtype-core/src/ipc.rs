use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::MAX_MESSAGE_BYTES;

pub const PIPE_NAME: &str = r"\\.\pipe\flowtype-input-v4";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InjectorRequest {
    Hello,
    BeginSession {
        session_id: String,
    },
    ApplyState {
        session_id: String,
        sequence: i64,
        full_text: String,
    },
    FinishSession {
        session_id: String,
        sequence: i64,
    },
    QuerySession {
        session_id: String,
    },
    ProbeTarget,
    CancelInvalidSession {
        session_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InjectorResponse {
    Hello {
        ipc_version: u16,
        instance_id: String,
        executable_path: String,
        elevated: bool,
    },
    SessionBegun {
        target_name: String,
    },
    Applied {
        sequence: i64,
    },
    Finished {
        sequence: i64,
    },
    SessionActive {
        session_id: String,
        sequence: i64,
        full_text: String,
    },
    SessionFinished {
        session_id: String,
        sequence: i64,
        full_text: String,
    },
    SessionMissing,
    TargetReady {
        target_name: String,
        activity_age_ms: u64,
    },
    Cancelled,
    TargetNotForeground {
        target_name: String,
    },
    TargetInvalid,
    TargetModified,
    TargetUnsupported,
    TsfUnavailable,
    InjectionUnknown,
    InvalidRequest,
}

pub fn write_message<T: Serialize>(stream: &mut impl Write, message: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(message).map_err(io::Error::other)?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    stream.write_all(&(payload.len() as u32).to_le_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()
}

pub fn read_message<T: DeserializeOwned>(stream: &mut impl Read) -> io::Result<T> {
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{InjectorRequest, InjectorResponse, read_message, write_message};

    #[test]
    fn round_trips_a_typed_message() {
        let expected = InjectorRequest::ApplyState {
            session_id: "session".to_owned(),
            sequence: 8,
            full_text: "中文\ntext".to_owned(),
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &expected).unwrap();
        assert_eq!(
            read_message::<InjectorRequest>(&mut Cursor::new(bytes)).unwrap(),
            expected,
        );
    }

    #[test]
    fn serializes_the_v4_handshake_and_session_query() {
        let hello = InjectorResponse::Hello {
            ipc_version: crate::INJECTOR_IPC_VERSION,
            instance_id: "injector-1".to_owned(),
            executable_path: r"C:\Program Files\FlowType\flowtype-injector.exe".to_owned(),
            elevated: true,
        };
        let query = InjectorRequest::QuerySession {
            session_id: "voice".to_owned(),
        };

        assert_eq!(serde_json::to_value(hello).unwrap()["type"], "hello");
        let mut bytes = Vec::new();
        write_message(&mut bytes, &query).unwrap();
        assert_eq!(
            read_message::<InjectorRequest>(&mut Cursor::new(bytes)).unwrap(),
            query,
        );
    }
}
