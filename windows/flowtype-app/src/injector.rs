use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
    TerminateProcess,
};

use flowtype_core::ipc::{
    InjectorRequest, InjectorResponse, PIPE_NAME, read_message, write_message,
};

pub struct InjectorClient(File);

impl InjectorClient {
    pub fn connect() -> io::Result<Self> {
        if let Ok(file) = open_pipe()
            && let Ok(client) = Self::connect_existing(file)
        {
            return Ok(client);
        }
        start_injector()?;
        for _ in 0..30 {
            if let Ok(file) = open_pipe()
                && let Ok(client) = Self::connect_existing(file)
            {
                return Ok(client);
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
            if let Ok(file) = open_pipe()
                && let Ok(client) = Self::connect_existing(file)
            {
                return Ok(client);
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "input service did not become ready",
        ))
    }

    fn connect_existing(file: File) -> io::Result<Self> {
        let server_pid = server_process_id(&file).ok();
        let mut client = Self(file);
        let response = client.request(InjectorRequest::QueryIdentity);
        match response {
            Ok(InjectorResponse::Identity {
                protocol_version,
                executable_path,
            }) if protocol_version == flowtype_core::PROTOCOL_VERSION
                && same_path(&executable_path, &injector_path()?) =>
            {
                Ok(client)
            }
            Ok(_) | Err(_) => {
                drop(client);
                terminate_stale_injector(server_pid);
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "input service identity mismatch",
                ))
            }
        }
    }
}

fn open_pipe() -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(PIPE_NAME)
}

fn start_injector() -> io::Result<()> {
    let sibling = injector_path()?;
    Command::new(sibling)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    Ok(())
}

fn injector_path() -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    Ok(current
        .parent()
        .map(|path| path.join("flowtype-injector.exe"))
        .unwrap_or_else(|| PathBuf::from("flowtype-injector.exe")))
}

fn same_path(actual: &str, expected: &std::path::Path) -> bool {
    std::path::Path::new(actual)
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
}

fn server_process_id(file: &File) -> io::Result<u32> {
    let mut process_id = 0_u32;
    if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle() as HANDLE, &mut process_id) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(process_id)
}

fn terminate_stale_injector(process_id: Option<u32>) {
    let Some(process_id) = process_id else {
        return;
    };
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            process_id,
        )
    };
    if process.is_null() {
        return;
    }
    let path = process_path(process).unwrap_or_default();
    let is_injector = std::path::Path::new(&path)
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("flowtype-injector.exe"));
    if is_injector {
        unsafe {
            let _ = TerminateProcess(process, 1);
        }
    }
    unsafe { CloseHandle(process) };
}

fn process_path(process: HANDLE) -> io::Result<String> {
    let mut path = vec![0_u16; 32_768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(String::from_utf16_lossy(&path[..length as usize]))
}
