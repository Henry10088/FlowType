use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn log(event: impl AsRef<str>) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let path = std::env::temp_dir().join("flowtype-app.log");
    let _ = flowtype_core::diagnostics::append_bounded(
        &path,
        &format!("{timestamp} {}", event.as_ref()),
        1024 * 1024,
    );
}
