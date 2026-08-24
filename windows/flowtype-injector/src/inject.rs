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

pub fn replace_text(previous: &str, next: &str) -> io::Result<()> {
    if let Some((delete_count, insert_text)) = tail_replacement(previous, next) {
        send_backspaces(delete_count)?;
        return send_text(&insert_text);
    }

    // A middle-of-text edit cannot be located through SendInput without
    // taking over the user's selection. Keep the conservative fallback for
    // that uncommon case.
    send_backspaces(previous.chars().count())?;
    send_text(next)
}

fn tail_replacement(previous: &str, next: &str) -> Option<(usize, String)> {
    if let Some(insert_text) = next.strip_prefix(previous) {
        return Some((0, insert_text.to_owned()));
    }
    if let Some(removed_text) = previous.strip_prefix(next) {
        return Some((removed_text.chars().count(), String::new()));
    }

    let previous_chars: Vec<char> = previous.chars().collect();
    let next_chars: Vec<char> = next.chars().collect();
    let prefix_len = previous_chars
        .iter()
        .zip(&next_chars)
        .take_while(|(left, right)| left == right)
        .count();
    let previous_tail = &previous_chars[prefix_len..];
    let next_tail = &next_chars[prefix_len..];

    // If a common suffix remains, the edit was in the middle and the cursor
    // position is no longer sufficient to apply it safely.
    let suffix_len = previous_tail
        .iter()
        .rev()
        .zip(next_tail.iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    (suffix_len == 0).then(|| (previous_tail.len(), next_tail.iter().collect::<String>()))
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

#[cfg(test)]
mod tests {
    use super::tail_replacement;

    #[test]
    fn appends_only_new_tail() {
        assert_eq!(
            tail_replacement("你好", "你好世界"),
            Some((0, "世界".to_owned()))
        );
    }

    #[test]
    fn trims_only_removed_tail() {
        assert_eq!(
            tail_replacement("你好世界", "你好"),
            Some((2, String::new()))
        );
    }

    #[test]
    fn replaces_changed_tail() {
        assert_eq!(
            tail_replacement("今天很好", "今天不错"),
            Some((2, "不错".to_owned()))
        );
    }

    #[test]
    fn rejects_middle_edit() {
        assert_eq!(tail_replacement("abcde", "abXde"), None);
    }
}
