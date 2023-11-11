//! Types and handles for plugins to interact with the host.

use clack_common::extensions::{Extension, HostExtensionSide};
use clack_common::utils::ClapVersion;
use clap_sys::host::clap_host;
use std::ffi::CStr;
use std::marker::PhantomData;
use std::ptr::NonNull;

/// Various information about the host, provided at plugin instantiation time.
#[derive(Copy, Clone)]
pub struct HostInfo<'a> {
    raw: *const clap_host,
    _lifetime: PhantomData<&'a clap_host>,
}

impl<'a> HostInfo<'a> {
    /// Creates a new [`HostInfo`] type from a given raw, C FFI compatible pointer.
    ///
    /// # Safety
    /// Pointer must be valid for the duration of the `'a` lifetime. Moreover, the contents of
    /// the `clap_host` struct must all also be valid.
    #[inline]
    pub unsafe fn from_raw(raw: *const clap_host) -> Self {
        Self {
            raw,
            _lifetime: PhantomData,
        }
    }

    /// The [`ClapVersion`] the host uses.
    #[inline]
    pub fn clap_version(&self) -> ClapVersion {
        ClapVersion::from_raw(self.as_raw().clap_version)
    }

    /// An user-friendly name for the host.
    ///
    /// This should always be set by the host.
    pub fn name(&self) -> Option<&'a CStr> {
        NonNull::new(self.as_raw().name as *mut _)
            .map(|ptr| unsafe { CStr::from_ptr(ptr.as_ptr()) })
    }

    /// The host's vendor.
    ///
    /// This field is optional.
    pub fn vendor(&self) -> Option<&'a CStr> {
        NonNull::new(self.as_raw().vendor as *mut _)
            .map(|ptr| unsafe { CStr::from_ptr(ptr.as_ptr()) })
    }

    /// An URL to the host's webpage.
    ///
    /// This field is optional.
    pub fn url(&self) -> Option<&'a CStr> {
        NonNull::new(self.as_raw().url as *mut _).map(|ptr| unsafe { CStr::from_ptr(ptr.as_ptr()) })
    }

    /// A version string for the host.
    ///
    /// This should always be set by the host.
    pub fn version(&self) -> Option<&'a CStr> {
        NonNull::new(self.as_raw().version as *mut _)
            .map(|ptr| unsafe { CStr::from_ptr(ptr.as_ptr()) })
    }

    pub fn get_extension<E: Extension<ExtensionSide = HostExtensionSide>>(&self) -> Option<&'a E> {
        let ext =
            unsafe { (self.as_raw().get_extension?)(self.raw, E::IDENTIFIER.as_ptr()) } as *mut _;
        NonNull::new(ext).map(|p| unsafe { E::from_extension_ptr(p) })
    }

    /// # Safety
    /// Some functions exposed by HostHandle cannot be called until plugin is initialized
    #[inline]
    pub(crate) unsafe fn to_handle(self) -> HostHandle<'a> {
        HostHandle {
            raw: self.raw,
            _lifetime: PhantomData,
        }
    }

    #[inline]
    pub fn as_raw(&self) -> &'a clap_host {
        unsafe { &*self.raw }
    }
}

/// A thread-safe handle to the host.
///
/// This can be used to fetch information about the host, scan available extensions, or perform
/// requests to the host.
#[derive(Copy, Clone)]
pub struct HostHandle<'a> {
    raw: *const clap_host,
    _lifetime: PhantomData<&'a clap_host>,
}

unsafe impl<'a> Send for HostHandle<'a> {}
unsafe impl<'a> Sync for HostHandle<'a> {}

impl<'a> HostHandle<'a> {
    /// Returns host information.
    #[inline]
    pub fn info(&self) -> HostInfo<'a> {
        HostInfo {
            raw: self.raw,
            _lifetime: PhantomData,
        }
    }

    /// Returns a raw, C FFI-compatible reference to the host handle.
    #[inline]
    pub fn as_raw(&self) -> &'a clap_host {
        unsafe { &*self.raw }
    }

    /// Requests the host to [deactivate](crate::plugin::PluginAudioProcessor::deactivate) and then
    /// [re-activate](crate::plugin::PluginAudioProcessor::activate) the plugin.
    /// The operation may be delayed by the host.
    #[inline]
    pub fn request_restart(&self) {
        if let Some(request_restart) = self.as_raw().request_restart {
            // SAFETY: field is guaranteed to be correct by host. Lifetime is enforced by 'a
            unsafe { request_restart(self.raw) }
        }
    }

    /// Requests the host to [activate](crate::plugin::PluginAudioProcessor::activate) the plugin,
    /// and start audio processing.
    /// This is useful if you have external IO and need to wake the plugin up from "sleep".
    #[inline]
    pub fn request_process(&self) {
        if let Some(request_process) = self.as_raw().request_process {
            // SAFETY: field is guaranteed to be correct by host. Lifetime is enforced by 'a
            unsafe { request_process(self.raw) }
        }
    }

    /// Requests the host to schedule a call to the
    /// [on_main_thread](crate::plugin::PluginMainThread::on_main_thread) method on the main thread.
    #[inline]
    pub fn request_callback(&self) {
        if let Some(request_callback) = self.as_raw().request_callback {
            // SAFETY: field is guaranteed to be correct by host. Lifetime is enforced by 'a
            unsafe { request_callback(self.raw) }
        }
    }

    #[inline]
    pub fn extension<E: Extension<ExtensionSide = HostExtensionSide>>(&self) -> Option<&'a E> {
        self.info().get_extension()
    }

    /// # Safety
    ///
    /// Callers *MUST* ensure this is only called on the main thread, and that they have exclusive (&mut) access.
    #[inline]
    pub unsafe fn as_main_thread_unchecked(&self) -> HostMainThreadHandle<'a> {
        HostMainThreadHandle {
            raw: self.raw,
            _lifetime: PhantomData,
        }
    }

    /// # Safety
    ///
    /// Callers *MUST* ensure this is only called on the audio thread, and that they have exclusive (&mut) access.
    #[inline]
    pub unsafe fn as_audio_thread_unchecked(&self) -> HostAudioThreadHandle<'a> {
        HostAudioThreadHandle {
            raw: self.raw,
            _lifetime: PhantomData,
        }
    }
}

impl<'a> From<HostHandle<'a>> for HostInfo<'a> {
    #[inline]
    fn from(h: HostHandle<'a>) -> Self {
        h.info()
    }
}

#[derive(Copy, Clone)]
pub struct HostMainThreadHandle<'a> {
    raw: *const clap_host,
    _lifetime: PhantomData<&'a clap_host>,
}

impl<'a> HostMainThreadHandle<'a> {
    #[inline]
    pub fn shared(&self) -> HostHandle<'a> {
        HostHandle {
            raw: self.raw,
            _lifetime: PhantomData,
        }
    }

    #[inline]
    pub fn as_raw(&self) -> &'a clap_host {
        unsafe { &*self.raw }
    }

    pub(crate) fn into_sublifetime<'s>(self) -> HostMainThreadHandle<'s> {
        HostMainThreadHandle {
            raw: self.raw,
            _lifetime: PhantomData,
        }
    }
}

impl<'a> From<HostMainThreadHandle<'a>> for HostHandle<'a> {
    #[inline]
    fn from(h: HostMainThreadHandle<'a>) -> Self {
        h.shared()
    }
}

#[derive(Copy, Clone)]
pub struct HostAudioThreadHandle<'a> {
    raw: *const clap_host,
    _lifetime: PhantomData<&'a clap_host>,
}

unsafe impl<'a> Send for HostAudioThreadHandle<'a> {}

impl<'a> HostAudioThreadHandle<'a> {
    #[inline]
    pub fn shared(&self) -> HostHandle<'a> {
        HostHandle {
            raw: self.raw,
            _lifetime: PhantomData,
        }
    }

    #[inline]
    pub fn as_raw(&self) -> &'a clap_host {
        unsafe { &*self.raw }
    }
}

impl<'a> From<HostAudioThreadHandle<'a>> for HostHandle<'a> {
    #[inline]
    fn from(h: HostAudioThreadHandle<'a>) -> Self {
        h.shared()
    }
}
