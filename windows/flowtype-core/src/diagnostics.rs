use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn append_bounded(path: &Path, line: &str, max_bytes: u64) -> io::Result<()> {
    if max_bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero log limit",
        ));
    }
    let _guard = LOG_LOCK
        .lock()
        .map_err(|_| io::Error::other("diagnostic log lock poisoned"))?;
    let line = truncate_utf8(line, max_bytes.saturating_sub(1) as usize);
    let incoming = line.len() as u64 + 1;
    let current = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if current.saturating_add(incoming) > max_bytes {
        rotate(path)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

fn rotate(path: &Path) -> io::Result<()> {
    let backup = backup_path(path);
    match fs::remove_file(&backup) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match fs::rename(path, &backup) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map(|_| ()),
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".1");
    path.with_file_name(name)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{append_bounded, backup_path};

    static NEXT_LOG: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn rotates_before_the_current_log_exceeds_its_limit() {
        let path = std::env::temp_dir().join(format!(
            "flowtype-bounded-log-{}-{}.log",
            std::process::id(),
            NEXT_LOG.fetch_add(1, Ordering::Relaxed),
        ));
        let backup = backup_path(&path);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&backup);

        append_bounded(&path, "1234567890", 16).unwrap();
        append_bounded(&path, "abcdefghij", 16).unwrap();

        assert!(fs::metadata(&path).unwrap().len() <= 16);
        assert!(fs::metadata(&backup).unwrap().len() <= 16);
        assert_eq!(fs::read_to_string(&path).unwrap(), "abcdefghij\n");
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(backup);
    }

    #[test]
    fn truncates_long_unicode_lines_on_a_character_boundary() {
        let path = std::env::temp_dir().join(format!(
            "flowtype-bounded-unicode-{}-{}.log",
            std::process::id(),
            NEXT_LOG.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_file(&path);

        append_bounded(&path, "中文中文", 8).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.is_char_boundary(content.len()));
        assert!(fs::metadata(&path).unwrap().len() <= 8);
        let _ = fs::remove_file(path);
    }
}
