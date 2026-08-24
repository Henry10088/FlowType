use std::io;
use std::mem::size_of;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
    VIRTUAL_KEY, VK_BACK, VK_RETURN,
};

const INPUT_CHUNK_SIZE: usize = 64;

pub fn send_backspaces(count: usize) -> io::Result<()> {
    let mut inputs = Vec::with_capacity(count.saturating_mul(2));
    for _ in 0..count {
        push_virtual_key(&mut inputs, VK_BACK);
    }
    send_all(&inputs)
}

pub fn send_text(text: &str) -> io::Result<()> {
    let mut inputs = Vec::with_capacity(text.encode_utf16().count().saturating_mul(2));
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                push_virtual_key(&mut inputs, VK_RETURN);
            }
            '\n' => push_virtual_key(&mut inputs, VK_RETURN),
            _ => {
                let mut encoded = [0_u16; 2];
                for unit in character.encode_utf16(&mut encoded) {
                    push_unicode_unit(&mut inputs, *unit);
                }
            }
        }
    }

    send_all(&inputs)
}

fn push_virtual_key(inputs: &mut Vec<INPUT>, key: VIRTUAL_KEY) {
    inputs.push(keyboard_input(key, 0, 0));
    inputs.push(keyboard_input(key, 0, KEYEVENTF_KEYUP));
}

fn push_unicode_unit(inputs: &mut Vec<INPUT>, unit: u16) {
    inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE));
    inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
}

fn keyboard_input(virtual_key: VIRTUAL_KEY, scan_code: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: virtual_key,
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_all(inputs: &[INPUT]) -> io::Result<()> {
    for chunk in inputs.chunks(INPUT_CHUNK_SIZE) {
        let sent = unsafe {
            SendInput(
                chunk.len() as u32,
                chunk.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        };
        if sent != chunk.len() as u32 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
