use std::ffi::CString;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, SendMessageW};

use crate::diagnostics;

const SCI_INSERTTEXT: u32 = 2003;
const SCI_GETCHARAT: u32 = 2007;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GETANCHOR: u32 = 2009;
const SCI_GOTOPOS: u32 = 2025;
const SCI_DELETERANGE: u32 = 2645;

pub(crate) struct ExistingSuffixReplacement {
    editor: HWND,
    start: usize,
    text: CString,
    committed: bool,
}

impl ExistingSuffixReplacement {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for ExistingSuffixReplacement {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        unsafe {
            SendMessageW(
                self.editor,
                SCI_INSERTTEXT,
                Some(WPARAM(self.start)),
                Some(LPARAM(self.text.as_ptr() as isize)),
            );
            SendMessageW(
                self.editor,
                SCI_GOTOPOS,
                Some(WPARAM(self.start + self.text.as_bytes().len())),
                Some(LPARAM(0)),
            );
        }
    }
}

/// Scintilla exposes only the active insertion span through TSF. Replace an
/// exact suffix so the following TSF composition can own the same text range.
pub(crate) fn replace_exact_suffix(text: &str) -> Option<ExistingSuffixReplacement> {
    let text = CString::new(text)
        .map_err(|_| diagnostics::log("attach host=nul_text"))
        .ok()?;
    if text.as_bytes().is_empty() {
        diagnostics::log("attach host=empty_text");
        return None;
    }

    let editor = unsafe { GetFocus() };
    if editor.is_invalid() || !has_window_class(editor, "Scintilla") {
        diagnostics::log("attach host=not_scintilla");
        return None;
    }

    let caret = scintilla_position(editor, SCI_GETCURRENTPOS)?;
    let anchor = scintilla_position(editor, SCI_GETANCHOR)?;
    if caret != anchor || caret < text.as_bytes().len() {
        diagnostics::log(format!(
            "attach host=selection_or_short bytes={} caret={caret} anchor={anchor}",
            text.as_bytes().len()
        ));
        return None;
    }
    let start = caret - text.as_bytes().len();
    for (offset, expected) in text.as_bytes().iter().copied().enumerate() {
        let actual = unsafe {
            SendMessageW(
                editor,
                SCI_GETCHARAT,
                Some(WPARAM(start + offset)),
                Some(LPARAM(0)),
            )
        };
        if scintilla_byte(actual.0) != expected {
            diagnostics::log(format!(
                "attach host=text_mismatch bytes={} offset={offset}",
                text.as_bytes().len()
            ));
            return None;
        }
    }

    unsafe {
        SendMessageW(
            editor,
            SCI_DELETERANGE,
            Some(WPARAM(start)),
            Some(LPARAM(text.as_bytes().len() as isize)),
        );
    }
    if scintilla_position(editor, SCI_GETCURRENTPOS) != Some(start)
        || scintilla_position(editor, SCI_GETANCHOR) != Some(start)
    {
        diagnostics::log(format!(
            "attach host=delete_failed bytes={}",
            text.as_bytes().len()
        ));
        return None;
    }

    let replacement = ExistingSuffixReplacement {
        editor,
        start,
        text,
        committed: false,
    };
    diagnostics::log(format!(
        "attach host=replaced bytes={}",
        replacement.text.as_bytes().len()
    ));
    Some(replacement)
}

fn scintilla_byte(result: isize) -> u8 {
    result as u8
}

fn scintilla_position(editor: HWND, message: u32) -> Option<usize> {
    let result = unsafe { SendMessageW(editor, message, Some(WPARAM(0)), Some(LPARAM(0))) };
    usize::try_from(result.0).ok()
}

fn has_window_class(window: HWND, expected: &str) -> bool {
    let mut class_name = [0_u16; 32];
    let length = unsafe { GetClassNameW(window, &mut class_name) };
    length > 0
        && String::from_utf16_lossy(&class_name[..length as usize]).eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::scintilla_byte;

    #[test]
    fn compares_scintilla_utf8_bytes_without_signed_extension() {
        assert_eq!(scintilla_byte(-23), 0xe9);
        assert_eq!(scintilla_byte(0x80), 0x80);
        assert_eq!(scintilla_byte(b'A' as isize), b'A');
    }
}
