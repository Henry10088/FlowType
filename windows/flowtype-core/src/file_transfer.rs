use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::PROTOCOL_VERSION;

pub const FILE_TRANSFER_VERSION: u16 = 2;
pub const MAX_PHONE_ID_BYTES: usize = 128;
pub const MAX_BATCH_ID_BYTES: usize = 64;
pub const MAX_ENTRY_ID_BYTES: usize = 64;
pub const MAX_RELATIVE_PATH_BYTES: usize = 4096;
pub const MAX_ROOT_SUMMARIES: usize = 8;
pub const MAX_ROOT_SUMMARY_BYTES: usize = 255;
pub const MAX_MANIFEST_LINE_BYTES: usize = 16 * 1024;
pub const MIN_TICKET_BYTES: usize = 32;
pub const MAX_TICKET_TEXT_BYTES: usize = 128;
pub const MAX_WIRE_INTEGER: u64 = i64::MAX as u64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileClientMessage {
    FileOffer(FileOffer),
    FileDecision(FileDecision),
    FileCancel(FileCancel),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileServerMessage {
    FileOffer(FileOffer),
    FileTicket(FileTicket),
    FileResult { batch_id: String, state: BatchState },
    FileError { batch_id: String, code: String },
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    PhoneToComputer,
    ComputerToPhone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchState {
    WaitingConfirmation,
    Queued,
    Preparing,
    Transferring,
    WaitingNetwork,
    NeedsReselection,
    Completed,
    PartialFailure,
    Canceled,
}

impl BatchState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::PartialFailure | Self::Canceled
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        match self {
            Self::WaitingConfirmation => matches!(next, Self::Queued | Self::Canceled),
            Self::Queued => matches!(
                next,
                Self::Preparing | Self::WaitingNetwork | Self::Canceled
            ),
            Self::Preparing => matches!(
                next,
                Self::Transferring
                    | Self::WaitingNetwork
                    | Self::NeedsReselection
                    | Self::PartialFailure
                    | Self::Canceled
            ),
            Self::Transferring => matches!(
                next,
                Self::WaitingNetwork
                    | Self::NeedsReselection
                    | Self::Completed
                    | Self::PartialFailure
                    | Self::Canceled
            ),
            Self::WaitingNetwork => matches!(
                next,
                Self::Preparing | Self::Transferring | Self::NeedsReselection | Self::Canceled
            ),
            Self::NeedsReselection => matches!(next, Self::Preparing | Self::Canceled),
            Self::Completed | Self::PartialFailure | Self::Canceled => false,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    Pending,
    Transferring,
    WaitingNetwork,
    NeedsReselection,
    Completed,
    Failed,
    Canceled,
}

impl TryFrom<u8> for EntryState {
    type Error = ValidationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Pending),
            1 => Ok(Self::Transferring),
            2 => Ok(Self::WaitingNetwork),
            3 => Ok(Self::NeedsReselection),
            4 => Ok(Self::Completed),
            5 => Ok(Self::Failed),
            6 => Ok(Self::Canceled),
            _ => Err(ValidationError::InvalidEntry),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOffer {
    pub protocol_version: u16,
    pub transfer_version: u16,
    pub phone_id: String,
    pub batch_id: String,
    pub direction: TransferDirection,
    pub file_count: u64,
    pub directory_count: u64,
    pub total_bytes: u64,
    #[serde(default)]
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDecision {
    pub protocol_version: u16,
    pub transfer_version: u16,
    pub batch_id: String,
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTicket {
    pub protocol_version: u16,
    pub transfer_version: u16,
    pub batch_id: String,
    pub data_port: u16,
    pub token: String,
    pub expires_at_epoch_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileCancel {
    pub protocol_version: u16,
    pub transfer_version: u16,
    pub batch_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub ordinal: u64,
    pub entry_id: String,
    pub relative_path: String,
    pub kind: EntryKind,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at_epoch_millis: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeEntry {
    pub entry_id: String,
    pub committed_offset: u64,
    pub state: EntryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    UnsupportedProtocol,
    InvalidIdentifier,
    InvalidSummary,
    InvalidTicket,
    InvalidPath,
    InvalidEntry,
    InvalidHash,
    ManifestLineTooLarge,
}

impl FileOffer {
    pub fn validate(&self, authenticated_phone_id: &str) -> Result<(), ValidationError> {
        validate_versions(self.protocol_version, self.transfer_version)?;
        if self.phone_id != authenticated_phone_id
            || !valid_identifier(&self.phone_id, MAX_PHONE_ID_BYTES)
            || !valid_identifier(&self.batch_id, MAX_BATCH_ID_BYTES)
            || (self.file_count == 0 && self.directory_count == 0)
            || self.file_count > MAX_WIRE_INTEGER
            || self.directory_count > MAX_WIRE_INTEGER
            || self.total_bytes > MAX_WIRE_INTEGER
            || self.roots.len() > MAX_ROOT_SUMMARIES
            || self.roots.iter().any(|root| {
                root.is_empty()
                    || root.len() > MAX_ROOT_SUMMARY_BYTES
                    || root.chars().any(char::is_control)
            })
        {
            return Err(ValidationError::InvalidSummary);
        }
        Ok(())
    }
}

impl FileDecision {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_versions(self.protocol_version, self.transfer_version)?;
        validate_batch_id(&self.batch_id)
    }
}

impl FileTicket {
    pub fn validate(&self, now_epoch_seconds: u64) -> Result<(), ValidationError> {
        validate_versions(self.protocol_version, self.transfer_version)?;
        validate_batch_id(&self.batch_id)?;
        let decoded_len =
            base64_url_decoded_len(&self.token).ok_or(ValidationError::InvalidTicket)?;
        if self.data_port == 0
            || decoded_len < MIN_TICKET_BYTES
            || self.token.len() > MAX_TICKET_TEXT_BYTES
            || self.expires_at_epoch_seconds > MAX_WIRE_INTEGER
            || self.expires_at_epoch_seconds <= now_epoch_seconds
        {
            return Err(ValidationError::InvalidTicket);
        }
        Ok(())
    }
}

impl FileCancel {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_versions(self.protocol_version, self.transfer_version)?;
        validate_batch_id(&self.batch_id)
    }
}

impl ManifestEntry {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.ordinal > MAX_WIRE_INTEGER
            || self.size > MAX_WIRE_INTEGER
            || !valid_identifier(&self.entry_id, MAX_ENTRY_ID_BYTES)
        {
            return Err(ValidationError::InvalidIdentifier);
        }
        validate_relative_path(&self.relative_path)?;
        if self.kind == EntryKind::Directory && self.size != 0 {
            return Err(ValidationError::InvalidEntry);
        }
        if self.mime_type.as_ref().is_some_and(|mime| {
            mime.is_empty() || mime.len() > 255 || mime.chars().any(char::is_control)
        }) {
            return Err(ValidationError::InvalidEntry);
        }
        if self.sha256.as_ref().is_some_and(|hash| !valid_sha256(hash)) {
            return Err(ValidationError::InvalidHash);
        }
        let line = serde_json::to_vec(self).map_err(|_| ValidationError::InvalidEntry)?;
        if line.len() > MAX_MANIFEST_LINE_BYTES {
            return Err(ValidationError::ManifestLineTooLarge);
        }
        Ok(())
    }
}

impl ResumeEntry {
    pub fn validate(&self, expected_size: u64) -> Result<(), ValidationError> {
        if self.committed_offset > MAX_WIRE_INTEGER
            || expected_size > MAX_WIRE_INTEGER
            || !valid_identifier(&self.entry_id, MAX_ENTRY_ID_BYTES)
            || self.committed_offset > expected_size
            || (self.state == EntryState::Completed && self.committed_offset != expected_size)
        {
            return Err(ValidationError::InvalidEntry);
        }
        Ok(())
    }
}

pub fn validate_relative_path(path: &str) -> Result<(), ValidationError> {
    if path.is_empty()
        || path.len() > MAX_RELATIVE_PATH_BYTES
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path
            .chars()
            .any(|value| value == '\0' || value.is_control())
    {
        return Err(ValidationError::InvalidPath);
    }
    let mut count = 0usize;
    for segment in path.split('/') {
        count += 1;
        if segment.is_empty()
            || matches!(segment, "." | "..")
            || segment.encode_utf16().count() > 255
        {
            return Err(ValidationError::InvalidPath);
        }
    }
    if count > 128 {
        return Err(ValidationError::InvalidPath);
    }
    Ok(())
}

pub fn sanitize_windows_relative_path(path: &str) -> Result<Vec<String>, ValidationError> {
    validate_relative_path(path)?;
    Ok(path.split('/').map(sanitize_windows_segment).collect())
}

pub fn sanitize_windows_segment(segment: &str) -> String {
    let mut result = String::new();
    for value in segment.chars() {
        if value.is_control()
            || matches!(value, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        {
            result.push('_');
        } else {
            result.push(value);
        }
    }
    while result.ends_with([' ', '.']) {
        result.pop();
    }
    if result.is_empty() || matches!(result.as_str(), "." | "..") {
        result = "untitled".to_owned();
    }
    if is_windows_reserved_name(&result) {
        result.push('_');
    }
    truncate_utf16(&result, 240)
}

pub struct WindowsNameReservation {
    used: HashSet<String>,
}

impl WindowsNameReservation {
    pub fn new(existing: impl IntoIterator<Item = String>) -> Self {
        Self {
            used: existing
                .into_iter()
                .map(|value| value.to_lowercase())
                .collect(),
        }
    }

    pub fn reserve(&mut self, desired: &str, directory: bool) -> String {
        let base = sanitize_windows_segment(desired);
        if self.used.insert(base.to_lowercase()) {
            return base;
        }
        for suffix in 1u32.. {
            let candidate = append_collision_suffix(&base, suffix, directory);
            if self.used.insert(candidate.to_lowercase()) {
                return candidate;
            }
        }
        unreachable!()
    }
}

fn append_collision_suffix(name: &str, number: u32, directory: bool) -> String {
    let suffix = format!(" ({number})");
    let (stem, extension) = if directory {
        (name, "")
    } else {
        match name.rsplit_once('.') {
            Some((stem, _extension)) if !stem.is_empty() => (stem, &name[stem.len()..]),
            _ => (name, ""),
        }
    };
    let allowed =
        240usize.saturating_sub(suffix.encode_utf16().count() + extension.encode_utf16().count());
    format!("{}{}{}", truncate_utf16(stem, allowed), suffix, extension)
}

fn validate_versions(protocol_version: u16, transfer_version: u16) -> Result<(), ValidationError> {
    if protocol_version != PROTOCOL_VERSION || transfer_version != FILE_TRANSFER_VERSION {
        Err(ValidationError::UnsupportedProtocol)
    } else {
        Ok(())
    }
}

pub fn validate_batch_id(value: &str) -> Result<(), ValidationError> {
    if valid_identifier(value, MAX_BATCH_ID_BYTES) {
        Ok(())
    } else {
        Err(ValidationError::InvalidIdentifier)
    }
}

fn valid_identifier(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn base64_url_decoded_len(value: &str) -> Option<usize> {
    if value.is_empty()
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'=')))
    {
        return None;
    }
    let padding = value.bytes().rev().take_while(|byte| *byte == b'=').count();
    if padding > 2 || value[..value.len().saturating_sub(padding)].contains('=') {
        return None;
    }
    Some(value.len().saturating_mul(3) / 4 - padding)
}

fn is_windows_reserved_name(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or(value).trim_end();
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn truncate_utf16(value: &str, max_units: usize) -> String {
    let mut units = 0usize;
    value
        .chars()
        .take_while(|character| {
            let next = units + character.len_utf16();
            if next > max_units {
                false
            } else {
                units = next;
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_offer_without_putting_the_manifest_on_wss() {
        let offer = FileOffer {
            protocol_version: PROTOCOL_VERSION,
            transfer_version: FILE_TRANSFER_VERSION,
            phone_id: "phone-1".into(),
            batch_id: "batch-1".into(),
            direction: TransferDirection::PhoneToComputer,
            file_count: 2,
            directory_count: 1,
            total_bytes: MAX_WIRE_INTEGER,
            roots: vec!["资料".into()],
        };
        offer.validate("phone-1").unwrap();
        let value = serde_json::to_value(FileClientMessage::FileOffer(offer)).unwrap();
        assert_eq!(value["type"], "file_offer");
        assert!(value.get("entries").is_none());
    }

    #[test]
    fn file_ticket_uses_the_tagged_control_message_contract() {
        let message = FileServerMessage::FileTicket(FileTicket {
            protocol_version: PROTOCOL_VERSION,
            transfer_version: FILE_TRANSFER_VERSION,
            batch_id: "batch-1".into(),
            data_port: 32188,
            token: "a".repeat(43),
            expires_at_epoch_seconds: 200,
        });
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["type"], "file_ticket");
    }

    #[test]
    fn rejects_unsigned_values_android_cannot_represent() {
        let offer = FileOffer {
            protocol_version: PROTOCOL_VERSION,
            transfer_version: FILE_TRANSFER_VERSION,
            phone_id: "phone-1".into(),
            batch_id: "batch-1".into(),
            direction: TransferDirection::PhoneToComputer,
            file_count: 1,
            directory_count: 0,
            total_bytes: MAX_WIRE_INTEGER + 1,
            roots: vec!["file.bin".into()],
        };
        assert_eq!(
            offer.validate("phone-1"),
            Err(ValidationError::InvalidSummary)
        );
    }

    #[test]
    fn rejects_traversal_and_windows_separator_in_wire_paths() {
        for path in ["../secret", "a/../secret", "/root", "a\\b", "a//b", "a/"] {
            assert_eq!(
                validate_relative_path(path),
                Err(ValidationError::InvalidPath)
            );
        }
    }

    #[test]
    fn sanitizes_android_names_for_windows() {
        assert_eq!(sanitize_windows_segment("CON.txt"), "CON.txt_");
        assert_eq!(
            sanitize_windows_segment("报告:终稿?.txt. "),
            "报告_终稿_.txt"
        );
        assert_eq!(
            sanitize_windows_relative_path("资料/报告:终稿?.txt").unwrap(),
            vec!["资料", "报告_终稿_.txt"]
        );
    }

    #[test]
    fn reserves_names_case_insensitively_and_keeps_extensions() {
        let mut names = WindowsNameReservation::new(["Report.txt".to_owned()]);
        assert_eq!(names.reserve("report.txt", false), "report (1).txt");
        assert_eq!(names.reserve("report.txt", false), "report (2).txt");
        assert_eq!(names.reserve("Photos", true), "Photos");
        assert_eq!(names.reserve("photos", true), "photos (1)");
    }

    #[test]
    fn validates_manifest_entries_and_resume_offsets() {
        let entry = ManifestEntry {
            ordinal: 0,
            entry_id: "entry-1".into(),
            relative_path: "资料/说明.txt".into(),
            kind: EntryKind::File,
            size: 42,
            modified_at_epoch_millis: Some(1),
            mime_type: Some("text/plain".into()),
            sha256: None,
        };
        entry.validate().unwrap();
        ResumeEntry {
            entry_id: entry.entry_id,
            committed_offset: 16,
            state: EntryState::WaitingNetwork,
        }
        .validate(42)
        .unwrap();
    }

    #[test]
    fn terminal_batch_states_cannot_restart() {
        assert!(BatchState::Queued.can_transition_to(BatchState::Preparing));
        assert!(BatchState::Transferring.can_transition_to(BatchState::WaitingNetwork));
        assert!(!BatchState::Completed.can_transition_to(BatchState::Transferring));
    }

    #[test]
    fn ticket_requires_a_long_lived_enough_random_secret() {
        let ticket = FileTicket {
            protocol_version: PROTOCOL_VERSION,
            transfer_version: FILE_TRANSFER_VERSION,
            batch_id: "batch-1".into(),
            data_port: 32188,
            token: "a".repeat(43),
            expires_at_epoch_seconds: 200,
        };
        ticket.validate(100).unwrap();
        assert_eq!(ticket.validate(200), Err(ValidationError::InvalidTicket));
    }
}
