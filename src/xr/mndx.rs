#![allow(dead_code)]
//! MNDX_xdev_space OpenXR extension wrapper
//!
//! This extension provides access to arbitrary XR device spaces in Monado,
//! allowing creation of XrSpaces for devices like trackers, controllers, and headsets.
//!
//! # Extension API
//!
//! The MNDX_xdev_space extension provides:
//! - Device enumeration via XDevList
//! - Device properties (name, serial, capabilities)
//! - Space creation for tracked devices
//!
//! # Example
//!
//! ```no_run
//! use openxr as xr;
//! use spacecal_for_monado::xr::mndx::Mndx;
//! # fn example<G>(instance: &xr::Instance, session: &xr::Session<G>) -> Result<(), Box<dyn std::error::Error>> {
//! let mndx = Mndx::new(instance)?;
//! let list = mndx.create_list(session)?;
//! let devices = list.enumerate_xdevs()?;
//!
//! for dev in devices {
//!     println!("Device: {} ({})", dev.name(), dev.serial());
//!     if dev.can_create_space() {
//!         let space = dev.create_space(session.clone())?;
//!         // Use space for tracking
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use openxr as xr;
use std::{ffi::CStr, mem::MaybeUninit, ptr, sync::Arc};

/// Extension name constant
pub const XDEV_SPACE_EXTENSION_NAME: &str = "XR_MNDX_xdev_space";

/// Main entry point for the MNDX_xdev_space extension
///
/// This struct holds the function pointers for all extension operations.
/// Create it using [`Mndx::new`] after enabling the extension on your OpenXR instance.
#[derive(Clone)]
pub struct Mndx {
    pfn: Arc<MndxPfn>,
}

/// Function pointers for MNDX extension operations
struct MndxPfn {
    xr_create_xdev_list: PFN_xrCreateXDevListMNDX,
    xr_destroy_xdev_list: PFN_xrDestroyXDevListMNDX,
    xr_enumerate_xdevs: PFN_xrEnumerateXDevsMNDX,
    xr_get_xdev_properties: PFN_xrGetXDevPropertiesMNDX,
    xr_get_xdev_list_generation_number: PFN_xrGetXDevListGenerationNumberMNDX,
    xr_create_xdev_space: PFN_xrCreateXDevSpaceMNDX,
}

impl Mndx {
    /// Load the MNDX_xdev_space extension from an OpenXR instance
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not available or if function pointers
    /// cannot be loaded.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use openxr as xr;
    /// # use spacecal_for_monado::xr::mndx::Mndx;
    /// # fn example(instance: &xr::Instance) -> Result<(), Box<dyn std::error::Error>> {
    /// let mndx = Mndx::new(instance)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(instance: &xr::Instance) -> Result<Self, MndxError> {
        let create_xdev_list: PFN_xrCreateXDevListMNDX =
            load_pfn(instance, b"xrCreateXDevListMNDX\0")?;

        let destroy_xdev_list: PFN_xrDestroyXDevListMNDX =
            load_pfn(instance, b"xrDestroyXDevListMNDX\0")?;

        let enumerate_xdevs: PFN_xrEnumerateXDevsMNDX =
            load_pfn(instance, b"xrEnumerateXDevsMNDX\0")?;

        let get_xdev_properties: PFN_xrGetXDevPropertiesMNDX =
            load_pfn(instance, b"xrGetXDevPropertiesMNDX\0")?;

        let get_xdev_list_generation_number: PFN_xrGetXDevListGenerationNumberMNDX =
            load_pfn(instance, b"xrGetXDevListGenerationNumberMNDX\0")?;

        let create_xdev_space: PFN_xrCreateXDevSpaceMNDX =
            load_pfn(instance, b"xrCreateXDevSpaceMNDX\0")?;

        Ok(Self {
            pfn: Arc::new(MndxPfn {
                xr_create_xdev_list: create_xdev_list,
                xr_destroy_xdev_list: destroy_xdev_list,
                xr_enumerate_xdevs: enumerate_xdevs,
                xr_get_xdev_properties: get_xdev_properties,
                xr_get_xdev_list_generation_number: get_xdev_list_generation_number,
                xr_create_xdev_space: create_xdev_space,
            }),
        })
    }

    /// Create a new XDevList for device enumeration
    ///
    /// The XDevList is used to query available XR devices and their properties.
    ///
    /// # Errors
    ///
    /// Returns an error if the list creation fails.
    pub fn create_list<G>(&self, session: &xr::Session<G>) -> Result<XDevList, MndxError> {
        let create_info = XrCreateXDevListInfoMNDX {
            type_: xr::sys::StructureType::from_raw(XR_TYPE_CREATE_XDEV_LIST_INFO_MNDX),
            next: ptr::null(),
        };

        let mut list: XrXDevListMNDX = unsafe { MaybeUninit::zeroed().assume_init() };

        let result = unsafe {
            (self.pfn.xr_create_xdev_list.unwrap())(session.as_raw(), &create_info, &mut list)
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::CreateListFailed(result));
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let inner = Arc::new(XDevListInner {
            id: list,
            mndx_pfn: self.pfn.clone(),
        });

        Ok(XDevList {
            inner,
            mndx: self.clone(),
        })
    }
}

/// A list of available XR devices
///
/// Use this to enumerate devices and query their properties.
/// The list is automatically destroyed when dropped.
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
    /// Enumerate all available XR devices
    ///
    /// Returns a vector of [`XDev`] objects representing each device.
    ///
    /// # Errors
    ///
    /// Returns an error if enumeration fails or if device properties cannot be queried.
    pub fn enumerate_xdevs(&self) -> Result<Vec<XDev>, MndxError> {
        let mut raw_xdevs = Vec::with_capacity(64);
        let mut xdev_count: u32 = 0;

        let result = unsafe {
            (self.mndx.pfn.xr_enumerate_xdevs.unwrap())(
                self.inner.id,
                raw_xdevs.capacity() as _,
                &mut xdev_count,
                raw_xdevs.as_mut_ptr(),
            )
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::EnumerateFailed(result));
        }

        unsafe {
            raw_xdevs.set_len(xdev_count as _);
        }

        let mut xdevs = Vec::with_capacity(raw_xdevs.len());
        for d in raw_xdevs.into_iter() {
            xdevs.push(XDev::new(self.clone(), d)?);
        }
        Ok(xdevs)
    }

    /// Get the generation number of the device list
    ///
    /// The generation number increments whenever devices are added or removed.
    /// Use this to detect when the device list has changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn get_generation_number(&self) -> Result<u64, MndxError> {
        let mut generation = 0;

        let result = unsafe {
            (self
                .inner
                .mndx_pfn
                .xr_get_xdev_list_generation_number
                .unwrap())(self.inner.id, &mut generation)
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::GetGenerationFailed(result));
        }

        Ok(generation)
    }
}

impl Drop for XDevListInner {
    fn drop(&mut self) {
        unsafe { (self.mndx_pfn.xr_destroy_xdev_list.unwrap())(self.id) };
    }
}

/// An XR device (tracker, controller, headset, etc.)
///
/// Provides access to device properties and space creation.
pub struct XDev {
    inner: Arc<XDevInner>,
    list: XDevList,
}

impl XDev {
    /// Get the device ID
    pub fn id(&self) -> XrXDevIdMNDX {
        self.inner.id
    }

    /// Get the device name
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Get the device serial number
    pub fn serial(&self) -> &str {
        &self.inner.serial
    }

    /// Check if a space can be created for this device
    ///
    /// Returns `true` if [`XDev::create_space`] is supported for this device.
    pub fn can_create_space(&self) -> bool {
        self.inner.can_create_space
    }
}

struct XDevInner {
    id: XrXDevIdMNDX,
    name: String,
    serial: String,
    can_create_space: bool,
}

impl XDev {
    /// Create a new XDev from an ID
    ///
    /// Queries the device properties from the runtime.
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
            (list.mndx.pfn.xr_get_xdev_properties.unwrap())(
                list.inner.id,
                &info,
                &mut properties,
            )
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
            inner: Arc::new(XDevInner {
                id,
                name,
                serial,
                can_create_space: properties.can_create_space.into(),
            }),
            list,
        })
    }

    /// Create an XrSpace for this device
    ///
    /// The space can be used to track the device's pose in the XR environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the device does not support space creation or if
    /// space creation fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use openxr as xr;
    /// # use spacecal_for_monado::xr::mndx::XDev;
    /// # fn example<G>(device: &XDev, session: xr::Session<G>) -> Result<(), Box<dyn std::error::Error>> {
    /// if device.can_create_space() {
    ///     let space = device.create_space(session)?;
    ///     // Use space for tracking
    /// }
    /// # Ok(())
    /// # }
    /// ```
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
            (self.list.mndx.pfn.xr_create_xdev_space.unwrap())(
                session.as_raw(),
                &create_info,
                &mut space,
            )
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(MndxError::CreateSpaceFailed(result));
        }

        Ok(unsafe { xr::Space::reference_from_raw(session, space) })
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when using the MNDX extension
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

    #[error("Failed to get generation number: {0:?}")]
    GetGenerationFailed(xr::sys::Result),

    #[error("Failed to create space: {0:?}")]
    CreateSpaceFailed(xr::sys::Result),

    #[error("Device name is not valid UTF-8")]
    InvalidDeviceName,

    #[error("Device serial is not valid UTF-8")]
    InvalidDeviceSerial,

    #[error("Space creation is not supported for this device")]
    SpaceCreationNotSupported,
}

// ============================================================================
// Raw FFI Types
// ============================================================================

/// Device ID type
pub type XrXDevIdMNDX = u64;

/// Opaque device list handle
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XrXDevListMNDX_T {
    _unused: [u8; 0],
}
pub type XrXDevListMNDX = *mut XrXDevListMNDX_T;

// Structure type constants (must match OpenXR registry)
const XR_TYPE_SYSTEM_XDEV_SPACE_PROPERTIES_MNDX: i32 = 1000444001;
const XR_TYPE_CREATE_XDEV_LIST_INFO_MNDX: i32 = 1000444002;
const XR_TYPE_GET_XDEV_INFO_MNDX: i32 = 1000444003;
const XR_TYPE_XDEV_PROPERTIES_MNDX: i32 = 1000444004;
const XR_TYPE_CREATE_XDEV_SPACE_INFO_MNDX: i32 = 1000444005;

/// System properties for MNDX extension
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XrSystemXDevSpacePropertiesMNDX {
    pub type_: xr::sys::StructureType,
    pub next: *mut ::std::os::raw::c_void,
    pub supports_xdev_space: xr::sys::Bool32,
}

/// Create info for XDevList
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XrCreateXDevListInfoMNDX {
    pub type_: xr::sys::StructureType,
    pub next: *const ::std::os::raw::c_void,
}

/// Info for querying device properties
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XrGetXDevInfoMNDX {
    pub type_: xr::sys::StructureType,
    pub next: *const ::std::os::raw::c_void,
    pub id: XrXDevIdMNDX,
}

/// Device properties
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XrXDevPropertiesMNDX {
    pub type_: xr::sys::StructureType,
    pub next: *mut ::std::os::raw::c_void,
    pub name: [u8; 256],
    pub serial: [u8; 256],
    pub can_create_space: xr::sys::Bool32,
}

/// Create info for XDev space
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XrCreateXDevSpaceInfoMNDX {
    pub type_: xr::sys::StructureType,
    pub next: *const ::std::os::raw::c_void,
    pub xdev_list: XrXDevListMNDX,
    pub id: XrXDevIdMNDX,
    pub offset: xr::Posef,
}

// ============================================================================
// Function Pointer Types
// ============================================================================

pub type PFN_xrCreateXDevListMNDX = ::std::option::Option<
    unsafe extern "C" fn(
        session: xr::sys::Session,
        info: *const XrCreateXDevListInfoMNDX,
        xdevList: *mut XrXDevListMNDX,
    ) -> xr::sys::Result,
>;

pub type PFN_xrGetXDevListGenerationNumberMNDX = ::std::option::Option<
    unsafe extern "C" fn(xdevList: XrXDevListMNDX, outGeneration: *mut u64) -> xr::sys::Result,
>;

pub type PFN_xrEnumerateXDevsMNDX = ::std::option::Option<
    unsafe extern "C" fn(
        xdevList: XrXDevListMNDX,
        xdevCapacityInput: u32,
        xdevCountOutput: *mut u32,
        xdevs: *mut XrXDevIdMNDX,
    ) -> xr::sys::Result,
>;

pub type PFN_xrGetXDevPropertiesMNDX = ::std::option::Option<
    unsafe extern "C" fn(
        xdevList: XrXDevListMNDX,
        info: *const XrGetXDevInfoMNDX,
        properties: *mut XrXDevPropertiesMNDX,
    ) -> xr::sys::Result,
>;

pub type PFN_xrDestroyXDevListMNDX =
    ::std::option::Option<unsafe extern "C" fn(xdevList: XrXDevListMNDX) -> xr::sys::Result>;

pub type PFN_xrCreateXDevSpaceMNDX = ::std::option::Option<
    unsafe extern "C" fn(
        session: xr::sys::Session,
        createInfo: *const XrCreateXDevSpaceInfoMNDX,
        space: *mut xr::sys::Space,
    ) -> xr::sys::Result,
>;

// ============================================================================
// Helper Functions
// ============================================================================

/// Load a function pointer from the OpenXR instance
///
/// # Safety
///
/// This function uses unsafe FFI calls to load function pointers.
fn load_pfn<T>(instance: &xr::Instance, name: &[u8]) -> Result<Option<T>, MndxError> {
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

    Ok(result)
}
