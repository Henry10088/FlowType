use std::fs;
use std::io;
use std::mem::size_of;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "FlowType";
const FLOATING_POSITION_FILE: &str = "floating-position-v1.txt";
const FLOATING_ENABLED_FILE: &str = "floating-enabled-v1.txt";

pub fn auto_start_enabled() -> bool {
    open_key(KEY_QUERY_VALUE)
        .map(|key| {
            let name = wide(VALUE_NAME);
            let mut value_type = 0;
            let mut bytes = 0;
            let result = unsafe {
                RegQueryValueExW(
                    key.0,
                    name.as_ptr(),
                    std::ptr::null_mut(),
                    &mut value_type,
                    std::ptr::null_mut(),
                    &mut bytes,
                )
            };
            result == ERROR_SUCCESS && value_type == REG_SZ && bytes >= size_of::<u16>() as u32
        })
        .unwrap_or(false)
}

pub fn set_auto_start(enabled: bool) -> io::Result<()> {
    let key = open_key(KEY_SET_VALUE)?;
    let name = wide(VALUE_NAME);
    if enabled {
        let executable = std::env::current_exe()?;
        let command = wide(&format!("\"{}\" --background", executable.display()));
        let bytes = (command.len() * size_of::<u16>()) as u32;
        let result = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                bytes,
            )
        };
        check(result)
    } else {
        let result = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
        if result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(result)
        }
    }
}

pub fn floating_position() -> Option<(i32, i32)> {
    let path = crate::identity::data_dir()
        .ok()?
        .join(FLOATING_POSITION_FILE);
    let value = fs::read_to_string(path).ok()?;
    let mut parts = value.trim().split(',');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

pub fn set_floating_position(position: (i32, i32)) -> io::Result<()> {
    let path = crate::identity::data_dir()?.join(FLOATING_POSITION_FILE);
    crate::atomic_file::write(&path, format!("{},{}", position.0, position.1).as_bytes())
}

pub fn floating_enabled() -> bool {
    let path = match crate::identity::data_dir() {
        Ok(path) => path.join(FLOATING_ENABLED_FILE),
        Err(_) => return true,
    };
    fs::read_to_string(path)
        .map(|value| value.trim() != "0")
        .unwrap_or(true)
}

pub fn set_floating_enabled(enabled: bool) -> io::Result<()> {
    let path = crate::identity::data_dir()?.join(FLOATING_ENABLED_FILE);
    crate::atomic_file::write(&path, if enabled { b"1" } else { b"0" })
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn open_key(access: u32) -> io::Result<RegistryKey> {
    let path = wide(RUN_KEY);
    let mut key = std::ptr::null_mut();
    let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    check(result)?;
    Ok(RegistryKey(key))
}

fn check(result: u32) -> io::Result<()> {
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
