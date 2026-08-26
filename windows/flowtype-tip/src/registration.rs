use std::mem::size_of;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HMODULE, RPC_E_CHANGED_MODE,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_TIP_SPEECH, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    ITfCategoryMgr, ITfInputProcessorProfileMgr,
};
use windows::core::{HRESULT, PCWSTR, Result};

use crate::{CLSID_FLOWTYPE_TIP, FLOWTYPE_LANG_ID, GUID_FLOWTYPE_PROFILE, module_instance};

const CLSID_TEXT: &str = "{9A50B266-9E86-4FF4-871B-8D47AD8C658B}";
const KEYBOARD_CATEGORY_TEXT: &str = "{34745C63-B2F0-4784-8B67-5E12C8701A31}";
const DESCRIPTION: &str = "FlowType Input Service";

pub fn register() -> Result<()> {
    register_com_server()?;
    if let Err(error) = register_tsf_profile() {
        let _ = unregister_com_server();
        return Err(error);
    }
    Ok(())
}

pub fn unregister() -> Result<()> {
    let tsf_result = unregister_tsf_profile();
    let com_result = unregister_com_server();
    tsf_result.and(com_result)
}

fn register_com_server() -> Result<()> {
    let path = module_path()?;
    let class_key = format!(r"Software\Classes\CLSID\{CLSID_TEXT}");
    set_registry_string(&class_key, None, "FlowType Speech Text Service")?;
    let server_key = format!(r"{class_key}\InProcServer32");
    set_registry_string(&server_key, None, &path)?;
    set_registry_string(&server_key, Some("ThreadingModel"), "Apartment")
}

fn unregister_com_server() -> Result<()> {
    let key = wide(&format!(r"Software\Classes\CLSID\{CLSID_TEXT}"));
    let status = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr())) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(win32_error(status.0))
    }
}

fn register_tsf_profile() -> Result<()> {
    let _com = ComScope::initialize()?;
    let profiles: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)? };
    let description: Vec<u16> = DESCRIPTION.encode_utf16().collect();
    let icon_path: Vec<u16> = module_path()?.encode_utf16().collect();
    unsafe {
        profiles.RegisterProfile(
            &CLSID_FLOWTYPE_TIP,
            FLOWTYPE_LANG_ID,
            &GUID_FLOWTYPE_PROFILE,
            &description,
            &icon_path,
            0,
            Default::default(),
            0,
            true,
            0,
        )?;
    }
    let categories: ITfCategoryMgr =
        unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)? };
    let _ = unsafe {
        categories.UnregisterCategory(
            &CLSID_FLOWTYPE_TIP,
            &GUID_TFCAT_TIP_KEYBOARD,
            &CLSID_FLOWTYPE_TIP,
        )
    };
    remove_keyboard_category_registry()?;
    for category in [
        GUID_TFCAT_TIP_SPEECH,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    ] {
        unsafe {
            categories.RegisterCategory(&CLSID_FLOWTYPE_TIP, &category, &CLSID_FLOWTYPE_TIP)?
        };
    }
    Ok(())
}

fn unregister_tsf_profile() -> Result<()> {
    let _com = ComScope::initialize().ok();
    if let Ok(categories) = unsafe {
        CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
    } {
        for category in [
            GUID_TFCAT_TIP_SPEECH,
            GUID_TFCAT_TIP_KEYBOARD,
            GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
            GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
        ] {
            let _ = unsafe {
                categories.UnregisterCategory(&CLSID_FLOWTYPE_TIP, &category, &CLSID_FLOWTYPE_TIP)
            };
        }
    }
    if let Ok(profiles) = unsafe {
        CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        )
    } {
        let _ = unsafe {
            profiles.UnregisterProfile(
                &CLSID_FLOWTYPE_TIP,
                FLOWTYPE_LANG_ID,
                &GUID_FLOWTYPE_PROFILE,
                0,
            )
        };
    }
    remove_tip_registry()
}

fn remove_keyboard_category_registry() -> Result<()> {
    let tip_root = format!(r"Software\Microsoft\CTF\TIP\{CLSID_TEXT}");
    delete_registry_tree(
        HKEY_LOCAL_MACHINE,
        &format!(r"{tip_root}\Category\Category\{KEYBOARD_CATEGORY_TEXT}"),
    )?;
    delete_registry_tree(
        HKEY_LOCAL_MACHINE,
        &format!(r"{tip_root}\Category\Item\{CLSID_TEXT}\{KEYBOARD_CATEGORY_TEXT}"),
    )
}

fn remove_tip_registry() -> Result<()> {
    let tip_root = format!(r"Software\Microsoft\CTF\TIP\{CLSID_TEXT}");
    delete_registry_tree(HKEY_LOCAL_MACHINE, &tip_root)?;
    delete_registry_tree(HKEY_CURRENT_USER, &tip_root)
}

fn module_path() -> Result<String> {
    let module = module_instance();
    if module == HMODULE::default() {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ));
    }
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe { GetModuleFileNameW(Some(module), &mut buffer) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(windows::core::Error::from_thread());
    }
    Ok(String::from_utf16_lossy(&buffer[..length]))
}

fn set_registry_string(path: &str, name: Option<&str>, value: &str) -> Result<()> {
    let path = wide(path);
    let name = name.map(wide);
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(win32_error(status.0));
    }
    let value = wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), value.len() * size_of::<u16>())
    };
    let status = unsafe {
        RegSetValueExW(
            key,
            name.as_ref()
                .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr())),
            None,
            REG_SZ,
            Some(bytes),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win32_error(status.0))
    }
}

fn delete_registry_tree(root: HKEY, path: &str) -> Result<()> {
    let key = wide(path);
    let status = unsafe { RegDeleteTreeW(root, PCWSTR(key.as_ptr())) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(win32_error(status.0))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn win32_error(code: u32) -> windows::core::Error {
    windows::core::Error::from_hresult(HRESULT::from_win32(code))
}

struct ComScope(bool);

impl ComScope {
    fn initialize() -> Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            Ok(Self(false))
        } else {
            result.ok()?;
            Ok(Self(true))
        }
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}
