pub mod diff;
pub mod ipc;
pub mod protocol;
pub mod session;
pub mod tip;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
