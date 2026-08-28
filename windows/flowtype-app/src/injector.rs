use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use flowtype_core::ipc::{
    InjectorRequest, InjectorResponse, PIPE_NAME, read_message, write_message,
};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, PeekNamedPipe};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
// The broker has enough time to time out one TIP edit and its cleanup before
// the app treats the whole service request as unresponsive.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const DEV_INJECTOR_ENV: &str = "FLOWTYPE_DEV_INJECTOR";

pub struct InjectorClient {
    pipe: File,
    instance_id: String,
}

impl InjectorClient {
    pub fn connect() -> io::Result<Self> {
        if let Ok(client) = open_verified_pipe() {
            return Ok(client);
        }

        activate_injector()?;
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        let mut last_error = io::Error::new(
            io::ErrorKind::NotConnected,
            "input service did not become ready",
        );
        while Instant::now() < deadline {
            match open_verified_pipe() {
                Ok(client) => return Ok(client),
                Err(error) => last_error = error,
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(last_error)
    }

    pub fn request(&mut self, request: InjectorRequest) -> io::Result<InjectorResponse> {
        write_message(&mut self.pipe, &request)?;
        read_response_with_timeout(&mut self.pipe, REQUEST_TIMEOUT)
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
}

fn open_verified_pipe() -> io::Result<InjectorClient> {
    let pipe = OpenOptions::new().read(true).write(true).open(PIPE_NAME)?;
    let server_path = server_process_path(&pipe)?;
    let mut client = InjectorClient {
        pipe,
        instance_id: String::new(),
    };
    let response = client.request(InjectorRequest::Hello)?;
    let InjectorResponse::Hello {
        ipc_version,
        instance_id,
        executable_path,
        elevated,
    } = response
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input service returned an invalid handshake",
        ));
    };
    let expected = injector_path()?;
    if ipc_version != flowtype_core::INJECTOR_IPC_VERSION
        || !same_path(&server_path, &expected)
        || !same_path(Path::new(&executable_path), &expected)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input service identity mismatch",
        ));
    }
    if !dev_injector_enabled() && !elevated {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "input service is not elevated",
        ));
    }
    if instance_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input service instance is missing",
        ));
    }
    client.instance_id = instance_id;
    Ok(client)
}

fn activate_injector() -> io::Result<()> {
    if dev_injector_enabled() {
        return start_sibling_injector();
    }
    let status = Command::new("schtasks.exe")
        .args(["/Run", "/TN", "FlowType Injector"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            "registered input service task could not be started",
        ))
    }
}

fn start_sibling_injector() -> io::Result<()> {
    Command::new(injector_path()?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

fn dev_injector_enabled() -> bool {
    std::env::var_os(DEV_INJECTOR_ENV).is_some_and(|value| value == "1")
}

fn injector_path() -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    Ok(current
        .parent()
        .map(|path| path.join("flowtype-injector.exe"))
        .unwrap_or_else(|| PathBuf::from("flowtype-injector.exe")))
}

fn same_path(actual: &Path, expected: &Path) -> bool {
    actual
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
}

fn server_process_path(file: &File) -> io::Result<PathBuf> {
    let mut process_id = 0_u32;
    if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle() as HANDLE, &mut process_id) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let result = process_path(process).map(PathBuf::from);
    unsafe { CloseHandle(process) };
    result
}

fn process_path(process: HANDLE) -> io::Result<String> {
    let mut path = vec![0_u16; 32_768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(String::from_utf16_lossy(&path[..length as usize]))
}

fn read_response_with_timeout(file: &mut File, timeout: Duration) -> io::Result<InjectorResponse> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut header = [0_u8; 4];
        let mut peeked = 0_u32;
        let mut available = 0_u32;
        if unsafe {
            PeekNamedPipe(
                file.as_raw_handle() as HANDLE,
                header.as_mut_ptr().cast(),
                header.len() as u32,
                &mut peeked,
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if peeked == header.len() as u32 {
            let payload_len = u32::from_le_bytes(header) as usize;
            if payload_len > flowtype_core::MAX_MESSAGE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "input service response is too large",
                ));
            }
            if available as usize >= header.len() + payload_len {
                return read_message(file);
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "input service response timed out",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::same_path;

    #[test]
    fn compares_windows_paths_case_insensitively() {
        assert!(same_path(
            Path::new(r"C:\Program Files\FlowType\flowtype-injector.exe"),
            Path::new(r"c:\program files\flowtype\FLOWTYPE-INJECTOR.EXE"),
        ));
    }
}
