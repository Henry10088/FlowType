#![allow(non_snake_case)]

mod composition;
mod factory;
mod ipc;
mod lifetime;
mod registration;
mod service;

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, HINSTANCE, HMODULE, S_FALSE, S_OK};
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::core::{BOOL, GUID, HRESULT};

pub const CLSID_FLOWTYPE_TIP: GUID = GUID::from_u128(flowtype_core::tip::CLSID_FLOWTYPE_TIP_VALUE);
pub const GUID_FLOWTYPE_PROFILE: GUID =
    GUID::from_u128(flowtype_core::tip::GUID_FLOWTYPE_PROFILE_VALUE);
pub const FLOWTYPE_LANG_ID: u16 = flowtype_core::tip::FLOWTYPE_LANG_ID;

static MODULE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn module_instance() -> HMODULE {
    HMODULE(MODULE.load(Ordering::Acquire))
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllMain(
    instance: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(instance.0, Ordering::Release);
        let _ = unsafe { DisableThreadLibraryCalls(HMODULE(instance.0)) };
    }
    true.into()
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    class_id: *const GUID,
    interface_id: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if class_id.is_null() || unsafe { *class_id } != CLSID_FLOWTYPE_TIP {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    unsafe { factory::query_factory(interface_id, object) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if lifetime::can_unload() {
        S_OK
    } else {
        S_FALSE
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
    registration::register()
        .map(|_| S_OK)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    registration::unregister()
        .map(|_| S_OK)
        .unwrap_or_else(|error| error.code())
}
