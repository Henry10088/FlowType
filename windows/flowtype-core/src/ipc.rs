use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::MAX_MESSAGE_BYTES;

pub const PIPE_NAME: &str = r"\\.\pipe\flowtype-input-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InjectorRequest {
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
    QueryStatus,
    QueryIdentity,
    ProbeTarget,
    CancelInvalidSession {
        session_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InjectorResponse {
    SessionBegun {
        target_name: String,
    },
    Applied {
        sequence: i64,
    },
    Finished {
        sequence: i64,
    },
    Ready,
    Identity {
        protocol_version: u16,
        executable_path: String,
    },
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

    use super::{InjectorRequest, read_message, write_message};

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
}
