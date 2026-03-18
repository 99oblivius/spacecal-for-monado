//! MNDX_xdev_space OpenXR extension wrapper for Monado device spaces.

#![allow(non_camel_case_types, non_snake_case)]

use openxr as xr;
use std::{ffi::CStr, mem::MaybeUninit, ptr, sync::Arc};

#[derive(Clone)]
pub struct Mndx {
    pfn: Arc<MndxPfn>,
}

struct MndxPfn {
    xr_create_xdev_list: unsafe extern "C" fn(xr::sys::Session, *const XrCreateXDevListInfoMNDX, *mut XrXDevListMNDX) -> xr::sys::Result,
    xr_destroy_xdev_list: unsafe extern "C" fn(XrXDevListMNDX) -> xr::sys::Result,
    xr_enumerate_xdevs: unsafe extern "C" fn(XrXDevListMNDX, u32, *mut u32, *mut XrXDevIdMNDX) -> xr::sys::Result,
    xr_get_xdev_properties: unsafe extern "C" fn(XrXDevListMNDX, *const XrGetXDevInfoMNDX, *mut XrXDevPropertiesMNDX) -> xr::sys::Result,
    xr_create_xdev_space: unsafe extern "C" fn(xr::sys::Session, *const XrCreateXDevSpaceInfoMNDX, *mut xr::sys::Space) -> xr::sys::Result,
}

impl Mndx {
    pub fn new(instance: &xr::Instance) -> Result<Self, MndxError> {
        // Validate all function pointers are non-null during construction
        // so callers never hit a None unwrap at runtime.
        let pfn = MndxPfn {
            xr_create_xdev_list: load_pfn(instance, b"xrCreateXDevListMNDX\0")?,
            xr_destroy_xdev_list: load_pfn(instance, b"xrDestroyXDevListMNDX\0")?,
            xr_enumerate_xdevs: load_pfn(instance, b"xrEnumerateXDevsMNDX\0")?,
            xr_get_xdev_properties: load_pfn(instance, b"xrGetXDevPropertiesMNDX\0")?,
            xr_create_xdev_space: load_pfn(instance, b"xrCreateXDevSpaceMNDX\0")?,
        };

        Ok(Self { pfn: Arc::new(pfn) })
    }

    pub fn create_list<G>(&self, session: &xr::Session<G>) -> Result<XDevList, MndxError> {
        let create_info = XrCreateXDevListInfoMNDX {
            type_: xr::sys::StructureType::from_raw(XR_TYPE_CREATE_XDEV_LIST_INFO_MNDX),
            next: ptr::null(),
        };

        let mut list: XrXDevListMNDX = unsafe { MaybeUninit::zeroed().assume_init() };

        let result = unsafe {
            (self.pfn.xr_create_xdev_list)(session.as_raw(), &create_info, &mut list)
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::CreateListFailed(result));
        }

        // Arc for shared ownership between XDevList and its XDev children.
        // XDevListInner is not Send (contains raw pointer) but never crosses threads.
        #[allow(clippy::arc_with_non_send_sync)]
        let inner = Arc::new(XDevListInner {
            id: list,
            mndx_pfn: self.pfn.clone(),
        });

        Ok(XDevList { inner, mndx: self.clone() })
    }
}

#[derive(Clone)]
pub struct XDevList {
    inner: Arc<XDevListInner>,
    mndx: Mndx,
}

struct XDevListInner {
    id: XrXDevListMNDX,
    mndx_pfn: Arc<MndxPfn>,
}

impl XDevList {
    pub fn enumerate_xdevs(&self) -> Result<Vec<XDev>, MndxError> {
        let mut raw_xdevs = Vec::with_capacity(64);
        let mut xdev_count: u32 = 0;

        let result = unsafe {
            (self.mndx.pfn.xr_enumerate_xdevs)(
                self.inner.id,
                raw_xdevs.capacity() as _,
                &mut xdev_count,
                raw_xdevs.as_mut_ptr(),
            )
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::EnumerateFailed(result));
        }

        unsafe { raw_xdevs.set_len(xdev_count as _); }

        raw_xdevs.into_iter()
            .map(|d| XDev::new(self.clone(), d))
            .collect()
    }

}

impl Drop for XDevListInner {
    fn drop(&mut self) {
        unsafe { (self.mndx_pfn.xr_destroy_xdev_list)(self.id) };
    }
}

pub struct XDev {
    inner: Arc<XDevInner>,
    list: XDevList,
}

struct XDevInner {
    id: XrXDevIdMNDX,
    name: String,
    serial: String,
    can_create_space: bool,
}

impl XDev {
    pub fn name(&self) -> &str { &self.inner.name }
    pub fn serial(&self) -> &str { &self.inner.serial }
    pub fn can_create_space(&self) -> bool { self.inner.can_create_space }

    fn new(list: XDevList, id: XrXDevIdMNDX) -> Result<Self, MndxError> {
        let info = XrGetXDevInfoMNDX {
            type_: xr::sys::StructureType::from_raw(XR_TYPE_GET_XDEV_INFO_MNDX),
            next: ptr::null(),
            id: id as _,
        };

        let mut properties = XrXDevPropertiesMNDX {
            type_: xr::sys::StructureType::from_raw(XR_TYPE_XDEV_PROPERTIES_MNDX),
            next: ptr::null_mut(),
            name: [0; 256],
            serial: [0; 256],
            can_create_space: xr::sys::Bool32::from_raw(0),
        };

        let result = unsafe {
            (list.mndx.pfn.xr_get_xdev_properties)(list.inner.id, &info, &mut properties)
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::GetPropertiesFailed(result));
        }

        let name = CStr::from_bytes_until_nul(&properties.name)
            .map_err(|_| MndxError::InvalidDeviceName)?
            .to_string_lossy()
            .into();
        let serial = CStr::from_bytes_until_nul(&properties.serial)
            .map_err(|_| MndxError::InvalidDeviceSerial)?
            .to_string_lossy()
            .into();

        Ok(XDev {
            inner: Arc::new(XDevInner { id, name, serial, can_create_space: properties.can_create_space.into() }),
            list,
        })
    }

    pub fn create_space<G>(&self, session: xr::Session<G>) -> Result<xr::Space, MndxError> {
        if !self.can_create_space() {
            return Err(MndxError::SpaceCreationNotSupported);
        }

        let create_info = XrCreateXDevSpaceInfoMNDX {
            type_: xr::sys::StructureType::from_raw(XR_TYPE_CREATE_XDEV_SPACE_INFO_MNDX),
            next: ptr::null(),
            xdev_list: self.list.inner.id,
            id: self.inner.id,
            offset: xr::Posef::IDENTITY,
        };

        let mut space: xr::sys::Space = unsafe { MaybeUninit::zeroed().assume_init() };

        let result = unsafe {
            (self.list.mndx.pfn.xr_create_xdev_space)(session.as_raw(), &create_info, &mut space)
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::CreateSpaceFailed(result));
        }

        Ok(unsafe { xr::Space::reference_from_raw(session, space) })
    }
}

// --- Errors ---

#[derive(Debug, thiserror::Error)]
pub enum MndxError {
    #[error("Failed to load extension function: {0}")]
    LoadFunctionFailed(String),
    #[error("Failed to create XDevList: {0:?}")]
    CreateListFailed(xr::sys::Result),
    #[error("Failed to enumerate devices: {0:?}")]
    EnumerateFailed(xr::sys::Result),
    #[error("Failed to get device properties: {0:?}")]
    GetPropertiesFailed(xr::sys::Result),
    #[error("Failed to create space: {0:?}")]
    CreateSpaceFailed(xr::sys::Result),
    #[error("Device name is not valid UTF-8")]
    InvalidDeviceName,
    #[error("Device serial is not valid UTF-8")]
    InvalidDeviceSerial,
    #[error("Space creation is not supported for this device")]
    SpaceCreationNotSupported,
}

// --- FFI types (ABI-required, not part of public API) ---

pub(crate) type XrXDevIdMNDX = u64;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub(crate) struct XrXDevListMNDX_T { _unused: [u8; 0] }
pub(crate) type XrXDevListMNDX = *mut XrXDevListMNDX_T;

const XR_TYPE_CREATE_XDEV_LIST_INFO_MNDX: i32 = 1000444002;
const XR_TYPE_GET_XDEV_INFO_MNDX: i32 = 1000444003;
const XR_TYPE_XDEV_PROPERTIES_MNDX: i32 = 1000444004;
const XR_TYPE_CREATE_XDEV_SPACE_INFO_MNDX: i32 = 1000444005;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct XrCreateXDevListInfoMNDX {
    type_: xr::sys::StructureType,
    next: *const ::std::os::raw::c_void,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct XrGetXDevInfoMNDX {
    type_: xr::sys::StructureType,
    next: *const ::std::os::raw::c_void,
    id: XrXDevIdMNDX,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct XrXDevPropertiesMNDX {
    type_: xr::sys::StructureType,
    next: *mut ::std::os::raw::c_void,
    name: [u8; 256],
    serial: [u8; 256],
    can_create_space: xr::sys::Bool32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct XrCreateXDevSpaceInfoMNDX {
    type_: xr::sys::StructureType,
    next: *const ::std::os::raw::c_void,
    xdev_list: XrXDevListMNDX,
    id: XrXDevIdMNDX,
    offset: xr::Posef,
}

// --- FFI loader ---

/// Loads a non-null function pointer from the OpenXR instance.
/// Option<fn> has guaranteed niche optimization (None = null pointer).
fn load_pfn<T>(instance: &xr::Instance, name: &[u8]) -> Result<T, MndxError> {
    let mut result: Option<T> = None;

    let load_result = unsafe {
        (instance.fp().get_instance_proc_addr)(
            instance.as_raw(),
            name.as_ptr() as _,
            &mut result as *mut Option<T> as *mut _,
        )
    };

    if load_result != xr::sys::Result::SUCCESS {
        return Err(MndxError::LoadFunctionFailed(
            String::from_utf8_lossy(name).into(),
        ));
    }

    result.ok_or_else(|| MndxError::LoadFunctionFailed(
        format!("{}: returned null", String::from_utf8_lossy(name)),
    ))
}
