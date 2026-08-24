use flowtype_core::tip::{CLSID_FLOWTYPE_TIP_VALUE, FLOWTYPE_LANG_ID, GUID_FLOWTYPE_PROFILE_VALUE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfileMgr, TF_INPUTPROCESSORPROFILE,
    TF_IPP_FLAG_ACTIVE, TF_IPP_FLAG_ENABLED, TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
    TF_IPPMF_ENABLEPROFILE, TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows::core::GUID;

#[cfg(test)]
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
#[cfg(test)]
use windows::Win32::UI::TextServices::GUID_TFCAT_TIP_KEYBOARD;

const CLSID_FLOWTYPE_TIP: GUID = GUID::from_u128(CLSID_FLOWTYPE_TIP_VALUE);
const GUID_FLOWTYPE_PROFILE: GUID = GUID::from_u128(GUID_FLOWTYPE_PROFILE_VALUE);
const ACTIVATE_FLAGS: u32 = TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE;

pub fn initialize_com() -> windows::core::Result<()> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }
}

pub fn ensure_flowtype_active() -> windows::core::Result<()> {
    let manager = profile_manager()?;
    let profile = flowtype_profile(&manager)?;
    if profile.dwFlags & TF_IPP_FLAG_ACTIVE != 0 {
        return Ok(());
    }
    let repair_disabled_profile = if profile.dwFlags & TF_IPP_FLAG_ENABLED == 0 {
        TF_IPPMF_ENABLEPROFILE
    } else {
        0
    };
    unsafe {
        manager.ActivateProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            FLOWTYPE_LANG_ID,
            &CLSID_FLOWTYPE_TIP,
            &GUID_FLOWTYPE_PROFILE,
            Default::default(),
            ACTIVATE_FLAGS | repair_disabled_profile,
        )?;
    }
    if flowtype_profile(&manager)?.dwFlags & TF_IPP_FLAG_ACTIVE == 0 {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ));
    }
    Ok(())
}

fn flowtype_profile(
    manager: &ITfInputProcessorProfileMgr,
) -> windows::core::Result<TF_INPUTPROCESSORPROFILE> {
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    unsafe {
        manager.GetProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            FLOWTYPE_LANG_ID,
            &CLSID_FLOWTYPE_TIP,
            &GUID_FLOWTYPE_PROFILE,
            Default::default(),
            &mut profile,
        )?;
    }
    Ok(profile)
}

fn profile_manager() -> windows::core::Result<ITfInputProcessorProfileMgr> {
    unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileIdentity {
    profile_type: u32,
    language_id: u16,
    class_id: GUID,
    profile_id: GUID,
    keyboard_layout: isize,
}

#[cfg(test)]
pub fn active_keyboard_profile() -> windows::core::Result<ProfileIdentity> {
    let manager = profile_manager()?;
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    unsafe { manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile)? };
    Ok(ProfileIdentity {
        profile_type: profile.dwProfileType,
        language_id: profile.langid,
        class_id: profile.clsid,
        profile_id: profile.guidProfile,
        keyboard_layout: profile.hkl.0 as isize,
    })
}

#[cfg(test)]
pub fn thread_keyboard_layout(thread_id: u32) -> isize {
    unsafe { GetKeyboardLayout(thread_id) }.0 as isize
}
