pub mod diff;
pub mod ipc;
pub mod protocol;
pub mod session;
pub mod tip;

/// Android <-> Windows WebSocket protocol.
pub const PROTOCOL_VERSION: u16 = 1;
/// Windows app <-> elevated injector protocol.
pub const INJECTOR_IPC_VERSION: u16 = 3;
/// Injector <-> in-process TSF text service protocol.
pub const TIP_IPC_VERSION: u16 = 3;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
