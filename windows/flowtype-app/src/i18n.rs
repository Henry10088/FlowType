use std::sync::OnceLock;

use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    Chinese,
    English,
}

static LANGUAGE: OnceLock<Language> = OnceLock::new();

fn current() -> Language {
    *LANGUAGE.get_or_init(|| language_from_id(unsafe { GetUserDefaultUILanguage() }))
}

const fn language_from_id(language_id: u16) -> Language {
    if language_id & 0x03ff == 0x0004 {
        Language::Chinese
    } else {
        Language::English
    }
}

pub fn tr(chinese: &'static str, english: &'static str) -> &'static str {
    match current() {
        Language::Chinese => chinese,
        Language::English => english,
    }
}

pub fn is_chinese() -> bool {
    current() == Language::Chinese
}

pub fn product_name() -> &'static str {
    tr("说写", "FlowType")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_all_chinese_sublanguages() {
        assert_eq!(language_from_id(0x0804), Language::Chinese);
        assert_eq!(language_from_id(0x0404), Language::Chinese);
        assert_eq!(language_from_id(0x0c04), Language::Chinese);
    }

    #[test]
    fn falls_back_to_english() {
        assert_eq!(language_from_id(0x0409), Language::English);
        assert_eq!(language_from_id(0x0411), Language::English);
    }
}
