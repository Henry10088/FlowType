pub mod diagnostics;
pub mod diff;
pub mod ipc;
pub mod protocol;
pub mod session;
pub mod tip;

/// Android <-> Windows WebSocket protocol.
pub const PROTOCOL_VERSION: u16 = 2;
/// Windows app <-> elevated injector protocol.
pub const INJECTOR_IPC_VERSION: u16 = 6;
/// Injector <-> in-process TSF text service protocol.
// Version 7 requires an independently cloned, gravity-anchored TSF range.
// Older components can collapse the owned range while moving the caret and
// then guess the replacement location from identical surrounding text.
pub const TIP_IPC_VERSION: u16 = 8;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
