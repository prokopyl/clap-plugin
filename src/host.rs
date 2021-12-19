use crate::extension::Extension;
use clap_sys::host::clap_host;
use clap_sys::version::clap_version;
use std::ffi::CStr;
use std::ptr::NonNull;

#[derive(Copy, Clone)]
pub struct HostInfo<'a> {
    pub(crate) inner: &'a clap_host,
}

impl<'a> HostInfo<'a> {
    #[inline]
    pub fn clap_version(&self) -> clap_version {
        self.inner.clap_version
    }

    pub fn name(&self) -> &'a str {
        let bytes = unsafe { CStr::from_ptr(self.inner.name) }.to_bytes();
        core::str::from_utf8(bytes).expect("Failed to read host name: invalid UTF-8 sequence")
    }

    pub fn vendor(&self) -> &'a str {
        let bytes = unsafe { CStr::from_ptr(self.inner.vendor) }.to_bytes();
        core::str::from_utf8(bytes).expect("Failed to read host name: invalid UTF-8 sequence")
    }

    pub fn url(&self) -> &'a str {
        let bytes = unsafe { CStr::from_ptr(self.inner.url) }.to_bytes();
        core::str::from_utf8(bytes).expect("Failed to read host name: invalid UTF-8 sequence")
    }

    pub fn version(&self) -> &'a str {
        let bytes = unsafe { CStr::from_ptr(self.inner.version) }.to_bytes();
        core::str::from_utf8(bytes).expect("Failed to read host name: invalid UTF-8 sequence")
    }

    pub fn get_extension<E: Extension<'a>>(&self) -> Option<E> {
        let ptr =
            unsafe { (self.inner.get_extension)(self.inner, E::IDENTIFIER as *const i8) } as *mut _;
        NonNull::new(ptr).map(|p| unsafe { E::from_extension_ptr(p) })
    }

    #[inline]
    pub(crate) unsafe fn to_handle(self) -> HostHandle<'a> {
        HostHandle { inner: self.inner }
    }
}

#[derive(Copy, Clone)]
pub struct HostHandle<'a> {
    inner: &'a clap_host,
}

impl<'a> HostHandle<'a> {
    #[inline]
    pub fn info(&self) -> HostInfo<'a> {
        HostInfo { inner: self.inner }
    }

    #[inline]
    pub fn request_restart(&self) {
        // SAFETY: field is guaranteed to be correct by host. Lifetime is enforced by 'a
        unsafe { (self.inner.request_restart)(self.inner) }
    }

    #[inline]
    pub fn request_process(&self) {
        // SAFETY: field is guaranteed to be correct by host. Lifetime is enforced by 'a
        unsafe { (self.inner.request_process)(self.inner) }
    }

    #[inline]
    pub fn request_callback(&self) {
        // SAFETY: field is guaranteed to be correct by host. Lifetime is enforced by 'a
        unsafe { (self.inner.request_callback)(self.inner) }
    }
}
