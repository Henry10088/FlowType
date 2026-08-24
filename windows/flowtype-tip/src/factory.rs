use std::ffi::c_void;

use windows::Win32::Foundation::CLASS_E_NOAGGREGATION;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::core::{ComObject, Interface, Ref, Result, implement};

use crate::lifetime::{ObjectGuard, set_server_lock};
use crate::service::TextService;

#[implement(IClassFactory)]
pub struct ClassFactory {
    _guard: ObjectGuard,
}

impl ClassFactory {
    pub fn new() -> Self {
        Self {
            _guard: ObjectGuard::new(),
        }
    }
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<windows::core::IUnknown>,
        interface_id: *const windows::core::GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        if !outer.is_null() {
            return Err(windows::core::Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        if interface_id.is_null() || object.is_null() {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_POINTER,
            ));
        }
        let service = ComObject::new(TextService::new());
        let unknown = service.into_interface::<windows::core::IUnknown>();
        unsafe { unknown.query(interface_id, object) }.ok()
    }

    fn LockServer(&self, locked: windows::core::BOOL) -> Result<()> {
        set_server_lock(locked.as_bool());
        Ok(())
    }
}

pub unsafe fn query_factory(
    interface_id: *const windows::core::GUID,
    object: *mut *mut c_void,
) -> windows::core::HRESULT {
    if interface_id.is_null() || object.is_null() {
        return windows::Win32::Foundation::E_POINTER;
    }
    let factory = ComObject::new(ClassFactory::new()).into_interface::<IClassFactory>();
    unsafe { factory.query(interface_id, object) }
}
