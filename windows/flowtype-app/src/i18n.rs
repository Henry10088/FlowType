use std::fs;
use std::io;
use std::sync::atomic::{AtomicU8, Ordering};

use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

const LANGUAGE_FILE: &str = "language-v1.txt";
const UNINITIALIZED: u8 = 0;
const CHINESE: u8 = 1;
const ENGLISH: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    Chinese,
    English,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageChoice {
    System,
    Chinese,
    English,
}

static LANGUAGE: AtomicU8 = AtomicU8::new(UNINITIALIZED);

fn current() -> Language {
    match LANGUAGE.load(Ordering::Acquire) {
        CHINESE => Language::Chinese,
        ENGLISH => Language::English,
        _ => {
            let language = resolve(language_choice());
            LANGUAGE.store(language_code(language), Ordering::Release);
            language
        }
    }
}

pub fn language_choice() -> LanguageChoice {
    crate::identity::data_dir()
        .ok()
        .and_then(|path| fs::read_to_string(path.join(LANGUAGE_FILE)).ok())
        .map(|value| parse_choice(value.trim()))
        .unwrap_or(LanguageChoice::System)
}

pub fn set_language_choice(choice: LanguageChoice) -> io::Result<()> {
    let path = crate::identity::data_dir()?.join(LANGUAGE_FILE);
    crate::atomic_file::write(&path, choice_value(choice).as_bytes())?;
    LANGUAGE.store(language_code(resolve(choice)), Ordering::Release);
    Ok(())
}

fn resolve(choice: LanguageChoice) -> Language {
    match choice {
        LanguageChoice::System => language_from_id(unsafe { GetUserDefaultUILanguage() }),
        LanguageChoice::Chinese => Language::Chinese,
        LanguageChoice::English => Language::English,
    }
}

const fn language_code(language: Language) -> u8 {
    match language {
        Language::Chinese => CHINESE,
        Language::English => ENGLISH,
    }
}

const fn language_from_id(language_id: u16) -> Language {
    if language_id & 0x03ff == 0x0004 {
        Language::Chinese
    } else {
        Language::English
    }
}

const fn choice_value(choice: LanguageChoice) -> &'static str {
    match choice {
        LanguageChoice::System => "system",
        LanguageChoice::Chinese => "zh-CN",
        LanguageChoice::English => "en",
    }
}

fn parse_choice(value: &str) -> LanguageChoice {
    match value {
        "zh-CN" => LanguageChoice::Chinese,
        "en" => LanguageChoice::English,
        _ => LanguageChoice::System,
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

    #[test]
    fn parses_saved_language_choices() {
        assert_eq!(parse_choice("zh-CN"), LanguageChoice::Chinese);
        assert_eq!(parse_choice("en"), LanguageChoice::English);
        assert_eq!(parse_choice("unknown"), LanguageChoice::System);
    }
}
