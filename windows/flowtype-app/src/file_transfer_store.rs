use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flowtype_core::file_transfer::{
    BatchState, EntryState, MAX_PHONE_ID_BYTES, MAX_WIRE_INTEGER, ManifestEntry, TransferDirection,
    validate_batch_id,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STORE_VERSION: u16 = 1;
const BATCH_FILE: &str = "batch.json";
const MANIFEST_FILE: &str = "manifest.ndjson";
const MANIFEST_INDEX_FILE: &str = "manifest-index-v1.bin";
const MANIFEST_COMPLETE_FILE: &str = "manifest-complete.json";
const PROGRESS_FILE: &str = "progress-v1.bin";
const PROGRESS_RECORD_BYTES: u64 = 16;
const PROGRESS_RECORD_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BatchRecord {
    pub(crate) store_version: u16,
    pub(crate) batch_id: String,
    pub(crate) phone_id: String,
    pub(crate) direction: TransferDirection,
    pub(crate) state: BatchState,
    pub(crate) created_at_epoch_seconds: u64,
    pub(crate) updated_at_epoch_seconds: u64,
    pub(crate) file_count: u64,
    pub(crate) directory_count: u64,
    pub(crate) total_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_root: Option<String>,
}

impl BatchRecord {
    pub(crate) fn new(
        batch_id: String,
        phone_id: String,
        direction: TransferDirection,
        now_epoch_seconds: u64,
        file_count: u64,
        directory_count: u64,
        total_bytes: u64,
    ) -> io::Result<Self> {
        let value = Self {
            store_version: STORE_VERSION,
            batch_id,
            phone_id,
            direction,
            state: match direction {
                TransferDirection::PhoneToComputer => BatchState::Queued,
                TransferDirection::ComputerToPhone => BatchState::WaitingConfirmation,
            },
            created_at_epoch_seconds: now_epoch_seconds,
            updated_at_epoch_seconds: now_epoch_seconds,
            file_count,
            directory_count,
            total_bytes,
            target_root: None,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> io::Result<()> {
        if self.store_version != STORE_VERSION
            || validate_batch_id(&self.batch_id).is_err()
            || self.phone_id.is_empty()
            || self.phone_id.len() > MAX_PHONE_ID_BYTES
            || !self
                .phone_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || (self.file_count == 0 && self.directory_count == 0)
            || self.file_count > MAX_WIRE_INTEGER
            || self.directory_count > MAX_WIRE_INTEGER
            || self.total_bytes > MAX_WIRE_INTEGER
            || self.updated_at_epoch_seconds < self.created_at_epoch_seconds
            || self.target_root.as_ref().is_some_and(|root| {
                root.is_empty() || root.len() > 32 * 1024 || root.chars().any(char::is_control)
            })
        {
            return Err(invalid_data("invalid file transfer batch record"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntryProgress {
    pub(crate) state: EntryState,
    pub(crate) committed_offset: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestComplete {
    store_version: u16,
    entry_count: u64,
    sha256: String,
}

pub(crate) struct FileTransferStore {
    root: PathBuf,
}

impl FileTransferStore {
    #[allow(dead_code)]
    pub(crate) fn load_default() -> io::Result<Self> {
        Self::new(crate::identity::data_dir()?.join("file-transfers-v1"))
    }

    pub(crate) fn new(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub(crate) fn create_batch(&self, batch: &BatchRecord) -> io::Result<()> {
        batch.validate()?;
        let directory = self.batch_dir(&batch.batch_id)?;
        fs::create_dir(&directory)?;
        crate::atomic_file::write(
            &directory.join(BATCH_FILE),
            &serde_json::to_vec(batch).map_err(json_error)?,
        )
    }

    pub(crate) fn load_batch(&self, batch_id: &str) -> io::Result<BatchRecord> {
        let path = self.batch_dir(batch_id)?.join(BATCH_FILE);
        let value: BatchRecord = serde_json::from_slice(&fs::read(path)?).map_err(json_error)?;
        value.validate()?;
        if value.batch_id != batch_id {
            return Err(invalid_data("batch directory and record do not match"));
        }
        Ok(value)
    }

    pub(crate) fn update_state(
        &self,
        batch_id: &str,
        state: BatchState,
        now_epoch_seconds: u64,
    ) -> io::Result<BatchRecord> {
        let mut batch = self.load_batch(batch_id)?;
        if !batch.state.can_transition_to(state) {
            return Err(invalid_data("invalid file transfer state transition"));
        }
        batch.state = state;
        batch.updated_at_epoch_seconds = now_epoch_seconds.max(batch.updated_at_epoch_seconds);
        batch.validate()?;
        crate::atomic_file::write(
            &self.batch_dir(batch_id)?.join(BATCH_FILE),
            &serde_json::to_vec(&batch).map_err(json_error)?,
        )?;
        Ok(batch)
    }

    pub(crate) fn begin_manifest(&self, batch_id: &str) -> io::Result<ManifestWriter> {
        let batch = self.load_batch(batch_id)?;
        let expected_entry_count = batch
            .file_count
            .checked_add(batch.directory_count)
            .ok_or_else(|| invalid_data("manifest entry count overflow"))?;
        let directory = self.batch_dir(batch_id)?;
        let path = directory.join(MANIFEST_FILE);
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let index = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(MANIFEST_INDEX_FILE))?;
        Ok(ManifestWriter {
            directory,
            file: BufWriter::with_capacity(512 * 1024, file),
            index: BufWriter::with_capacity(64 * 1024, index),
            next_ordinal: 0,
            expected_entry_count,
            next_offset: 0,
            hasher: Sha256::new(),
            committed: false,
        })
    }

    pub(crate) fn for_each_manifest_entry(
        &self,
        batch_id: &str,
        mut visit: impl FnMut(ManifestEntry) -> io::Result<()>,
    ) -> io::Result<u64> {
        let directory = self.batch_dir(batch_id)?;
        let completed = read_manifest_complete(&directory)?;
        let file = File::open(directory.join(MANIFEST_FILE))?;
        let mut reader = BufReader::with_capacity(512 * 1024, file);
        let mut buffer = Vec::new();
        let mut hasher = Sha256::new();
        let mut ordinal = 0u64;
        loop {
            buffer.clear();
            let read = reader.read_until(b'\n', &mut buffer)?;
            if read == 0 {
                break;
            }
            if buffer.last() != Some(&b'\n') {
                return Err(invalid_data("incomplete manifest line"));
            }
            hasher.update(&buffer);
            buffer.pop();
            let entry: ManifestEntry = serde_json::from_slice(&buffer).map_err(json_error)?;
            entry
                .validate()
                .map_err(|_| invalid_data("invalid manifest entry"))?;
            if entry.ordinal != ordinal {
                return Err(invalid_data("manifest ordinals are not contiguous"));
            }
            visit(entry)?;
            ordinal += 1;
        }
        let actual_hash = format!("{:x}", hasher.finalize());
        if ordinal != completed.entry_count || actual_hash != completed.sha256 {
            return Err(invalid_data("manifest completion record does not match"));
        }
        Ok(ordinal)
    }

    pub(crate) fn write_progress(
        &self,
        batch_id: &str,
        ordinal: u64,
        progress: EntryProgress,
    ) -> io::Result<()> {
        let directory = self.batch_dir(batch_id)?;
        let completed = read_manifest_complete(&directory)?;
        if ordinal >= completed.entry_count {
            return Err(invalid_data("progress ordinal is outside the manifest"));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.join(PROGRESS_FILE))?;
        file.seek(SeekFrom::Start(
            ordinal
                .checked_mul(PROGRESS_RECORD_BYTES)
                .ok_or_else(|| invalid_data("progress offset overflow"))?,
        ))?;
        file.write_all(&encode_progress(progress))?;
        file.sync_data()
    }

    pub(crate) fn manifest_entry(&self, batch_id: &str, ordinal: u64) -> io::Result<ManifestEntry> {
        let directory = self.batch_dir(batch_id)?;
        let completed = read_manifest_complete(&directory)?;
        if ordinal >= completed.entry_count {
            return Err(invalid_data("manifest ordinal is outside the batch"));
        }
        let mut index = File::open(directory.join(MANIFEST_INDEX_FILE))?;
        index.seek(SeekFrom::Start(
            ordinal
                .checked_mul(8)
                .ok_or_else(|| invalid_data("manifest index offset overflow"))?,
        ))?;
        let mut encoded_offset = [0u8; 8];
        index.read_exact(&mut encoded_offset)?;
        let mut manifest = BufReader::new(File::open(directory.join(MANIFEST_FILE))?);
        manifest.seek(SeekFrom::Start(u64::from_le_bytes(encoded_offset)))?;
        let mut line = Vec::new();
        manifest.read_until(b'\n', &mut line)?;
        if line.pop() != Some(b'\n') {
            return Err(invalid_data("incomplete manifest line"));
        }
        let entry: ManifestEntry = serde_json::from_slice(&line).map_err(json_error)?;
        entry
            .validate()
            .map_err(|_| invalid_data("invalid manifest entry"))?;
        if entry.ordinal != ordinal {
            return Err(invalid_data("manifest index does not match entry"));
        }
        Ok(entry)
    }

    pub(crate) fn read_progress(&self, batch_id: &str, ordinal: u64) -> io::Result<EntryProgress> {
        let directory = self.batch_dir(batch_id)?;
        let completed = read_manifest_complete(&directory)?;
        if ordinal >= completed.entry_count {
            return Err(invalid_data("progress ordinal is outside the manifest"));
        }
        let mut file = File::open(directory.join(PROGRESS_FILE))?;
        file.seek(SeekFrom::Start(
            ordinal
                .checked_mul(PROGRESS_RECORD_BYTES)
                .ok_or_else(|| invalid_data("progress offset overflow"))?,
        ))?;
        let mut record = [0u8; PROGRESS_RECORD_BYTES as usize];
        let mut filled = 0usize;
        while filled < record.len() {
            let read = file.read(&mut record[filled..])?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled != 0 && filled != record.len() {
            return Err(invalid_data("incomplete progress record"));
        }
        decode_progress(record)
    }

    pub(crate) fn recoverable_batches(&self) -> io::Result<Vec<BatchRecord>> {
        let mut batches = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(batch_id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(batch) = self.load_batch(&batch_id) else {
                continue;
            };
            if !batch.state.is_terminal() {
                batches.push(batch);
            }
        }
        batches.sort_by_key(|batch| (batch.created_at_epoch_seconds, batch.batch_id.clone()));
        Ok(batches)
    }

    fn batch_dir(&self, batch_id: &str) -> io::Result<PathBuf> {
        validate_batch_id(batch_id).map_err(|_| invalid_data("invalid batch id"))?;
        Ok(self.root.join(batch_id))
    }
}

pub(crate) struct ManifestWriter {
    directory: PathBuf,
    file: BufWriter<File>,
    index: BufWriter<File>,
    next_ordinal: u64,
    expected_entry_count: u64,
    next_offset: u64,
    hasher: Sha256,
    committed: bool,
}

impl ManifestWriter {
    pub(crate) fn append(&mut self, entry: &ManifestEntry) -> io::Result<()> {
        entry
            .validate()
            .map_err(|_| invalid_data("invalid manifest entry"))?;
        if entry.ordinal != self.next_ordinal {
            return Err(invalid_data("manifest ordinals must be contiguous"));
        }
        let mut line = serde_json::to_vec(entry).map_err(json_error)?;
        line.push(b'\n');
        self.index.write_all(&self.next_offset.to_le_bytes())?;
        self.file.write_all(&line)?;
        self.hasher.update(&line);
        self.next_offset = self
            .next_offset
            .checked_add(line.len() as u64)
            .ok_or_else(|| invalid_data("manifest offset overflow"))?;
        self.next_ordinal += 1;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> io::Result<u64> {
        if self.next_ordinal != self.expected_entry_count {
            return Err(invalid_data("manifest entry count does not match batch"));
        }
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        self.index.flush()?;
        self.index.get_ref().sync_all()?;
        let completed = ManifestComplete {
            store_version: STORE_VERSION,
            entry_count: self.next_ordinal,
            sha256: format!("{:x}", self.hasher.clone().finalize()),
        };
        initialize_progress(&self.directory, self.next_ordinal)?;
        crate::atomic_file::write(
            &self.directory.join(MANIFEST_COMPLETE_FILE),
            &serde_json::to_vec(&completed).map_err(json_error)?,
        )?;
        self.committed = true;
        Ok(self.next_ordinal)
    }
}

impl Drop for ManifestWriter {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(self.directory.join(MANIFEST_FILE));
            let _ = fs::remove_file(self.directory.join(MANIFEST_INDEX_FILE));
            let _ = fs::remove_file(self.directory.join(PROGRESS_FILE));
            let _ = fs::remove_file(self.directory.join(MANIFEST_COMPLETE_FILE));
        }
    }
}

fn initialize_progress(directory: &Path, entry_count: u64) -> io::Result<()> {
    entry_count
        .checked_mul(PROGRESS_RECORD_BYTES)
        .ok_or_else(|| invalid_data("progress file size overflow"))?;
    let path = directory.join(PROGRESS_FILE);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .sync_all()
}

fn read_manifest_complete(directory: &Path) -> io::Result<ManifestComplete> {
    let value: ManifestComplete =
        serde_json::from_slice(&fs::read(directory.join(MANIFEST_COMPLETE_FILE))?)
            .map_err(json_error)?;
    if value.store_version != STORE_VERSION
        || value.sha256.len() != 64
        || !value.sha256.bytes().all(|value| value.is_ascii_hexdigit())
    {
        return Err(invalid_data("invalid manifest completion record"));
    }
    Ok(value)
}

fn encode_progress(progress: EntryProgress) -> [u8; PROGRESS_RECORD_BYTES as usize] {
    let mut record = [0u8; PROGRESS_RECORD_BYTES as usize];
    record[0] = PROGRESS_RECORD_VERSION;
    record[1] = progress.state as u8;
    record[8..16].copy_from_slice(&progress.committed_offset.to_le_bytes());
    record
}

fn decode_progress(record: [u8; PROGRESS_RECORD_BYTES as usize]) -> io::Result<EntryProgress> {
    if record.iter().all(|value| *value == 0) {
        return Ok(EntryProgress {
            state: EntryState::Pending,
            committed_offset: 0,
        });
    }
    if record[0] != PROGRESS_RECORD_VERSION || record[2..8].iter().any(|value| *value != 0) {
        return Err(invalid_data("invalid progress record"));
    }
    let mut offset = [0u8; 8];
    offset.copy_from_slice(&record[8..16]);
    Ok(EntryProgress {
        state: EntryState::try_from(record[1])
            .map_err(|_| invalid_data("invalid progress state"))?,
        committed_offset: u64::from_le_bytes(offset),
    })
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowtype_core::file_transfer::EntryKind;

    #[test]
    fn streams_a_manifest_and_updates_fixed_progress_records() {
        let root = test_root("round-trip");
        let store = FileTransferStore::new(root.clone()).unwrap();
        let batch = BatchRecord::new(
            "batch-1".into(),
            "phone-1".into(),
            TransferDirection::PhoneToComputer,
            10,
            1,
            1,
            42,
        )
        .unwrap();
        store.create_batch(&batch).unwrap();
        let mut manifest = store.begin_manifest(&batch.batch_id).unwrap();
        manifest
            .append(&entry(0, "资料", EntryKind::Directory, 0))
            .unwrap();
        manifest
            .append(&entry(1, "资料/说明.txt", EntryKind::File, 42))
            .unwrap();
        assert_eq!(manifest.finish().unwrap(), 2);

        let mut paths = Vec::new();
        assert_eq!(
            store
                .for_each_manifest_entry(&batch.batch_id, |entry| {
                    paths.push(entry.relative_path);
                    Ok(())
                })
                .unwrap(),
            2
        );
        assert_eq!(paths, ["资料", "资料/说明.txt"]);
        assert_eq!(
            store
                .manifest_entry(&batch.batch_id, 1)
                .unwrap()
                .relative_path,
            "资料/说明.txt"
        );
        assert_eq!(
            store.read_progress(&batch.batch_id, 1).unwrap(),
            EntryProgress {
                state: EntryState::Pending,
                committed_offset: 0,
            }
        );
        store
            .write_progress(
                &batch.batch_id,
                1,
                EntryProgress {
                    state: EntryState::WaitingNetwork,
                    committed_offset: 16,
                },
            )
            .unwrap();
        assert_eq!(
            store
                .read_progress(&batch.batch_id, 1)
                .unwrap()
                .committed_offset,
            16
        );
        store
            .update_state(&batch.batch_id, BatchState::Preparing, 11)
            .unwrap();
        store
            .update_state(&batch.batch_id, BatchState::WaitingNetwork, 12)
            .unwrap();
        assert_eq!(store.recoverable_batches().unwrap().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_non_contiguous_manifest_ordinals_and_terminal_restarts() {
        let root = test_root("invalid");
        let store = FileTransferStore::new(root.clone()).unwrap();
        let batch = BatchRecord::new(
            "batch-2".into(),
            "phone-1".into(),
            TransferDirection::PhoneToComputer,
            10,
            1,
            0,
            42,
        )
        .unwrap();
        store.create_batch(&batch).unwrap();
        let mut manifest = store.begin_manifest(&batch.batch_id).unwrap();
        assert!(
            manifest
                .append(&entry(1, "a.txt", EntryKind::File, 42))
                .is_err()
        );
        drop(manifest);
        assert!(
            store
                .for_each_manifest_entry(&batch.batch_id, |_| Ok(()))
                .is_err()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_a_manifest_that_does_not_match_the_offered_count() {
        let root = test_root("count-mismatch");
        let store = FileTransferStore::new(root.clone()).unwrap();
        let batch = BatchRecord::new(
            "batch-3".into(),
            "phone-1".into(),
            TransferDirection::PhoneToComputer,
            10,
            2,
            0,
            42,
        )
        .unwrap();
        store.create_batch(&batch).unwrap();
        let mut manifest = store.begin_manifest(&batch.batch_id).unwrap();
        manifest
            .append(&entry(0, "a.txt", EntryKind::File, 42))
            .unwrap();
        assert!(manifest.finish().is_err());

        fs::remove_dir_all(root).unwrap();
    }

    fn entry(ordinal: u64, path: &str, kind: EntryKind, size: u64) -> ManifestEntry {
        ManifestEntry {
            ordinal,
            entry_id: format!("entry-{ordinal}"),
            relative_path: path.into(),
            kind,
            size,
            modified_at_epoch_millis: None,
            mime_type: None,
            sha256: None,
        }
    }

    fn test_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "flowtype-file-store-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        path
    }
}
