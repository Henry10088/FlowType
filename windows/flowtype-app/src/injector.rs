use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use flowtype_core::ipc::{
    InjectorRequest, InjectorResponse, PIPE_NAME, read_message, write_message,
};

pub struct InjectorClient(File);

impl InjectorClient {
    pub fn connect() -> io::Result<Self> {
        if let Ok(file) = open_pipe() {
            return Ok(Self(file));
        }
        start_injector()?;
        for _ in 0..30 {
            if let Ok(file) = open_pipe() {
                return Ok(Self(file));
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "input service did not start",
        ))
    }

    pub fn request(&mut self, request: InjectorRequest) -> io::Result<InjectorResponse> {
        write_message(&mut self.0, &request)?;
        let response = read_message(&mut self.0)?;
        Ok(response)
    }

    pub fn repair() -> io::Result<Self> {
        let status = Command::new("schtasks.exe")
            .args(["/Run", "/TN", "FlowType Injector"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;
        if !status.success() {
            return Err(io::Error::other(
                "registered input service task could not be started",
            ));
        }
        for _ in 0..30 {
            if let Ok(file) = open_pipe() {
                return Ok(Self(file));
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "input service did not become ready",
        ))
    }
}

fn open_pipe() -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(PIPE_NAME)
}

fn start_injector() -> io::Result<()> {
    let current = std::env::current_exe()?;
    let sibling = current
        .parent()
        .map(|path| path.join("flowtype-injector.exe"))
        .unwrap_or_else(|| PathBuf::from("flowtype-injector.exe"));
    Command::new(sibling)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    Ok(())
}
