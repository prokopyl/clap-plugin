//! Utilities to manipulate Plugin instances from an FFI context.
//!
//! These unsafe utilities are targeted at extension implementors. Most `clack-plugin` users do not
//! have to use those utilities to use extensions, see `clack-extensions` instead.

use crate::host::{HostAudioThreadHandle, HostHandle, HostMainThreadHandle};
use crate::plugin::{
    logging, AudioConfiguration, Plugin, PluginAudioProcessor, PluginBoxInner, PluginError,
    PluginMainThread, PluginShared,
};
use clap_sys::ext::log::*;
use clap_sys::plugin::clap_plugin;
use selfie::refs::RefType;
use selfie::Selfie;
use std::cell::UnsafeCell;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;

pub(crate) mod panic {
    #[cfg(not(test))]
    #[allow(unused)]
    pub use std::panic::catch_unwind;

    #[cfg(test)]
    #[inline]
    #[allow(unused)]
    pub fn catch_unwind<F: FnOnce() -> R + std::panic::UnwindSafe, R>(
        f: F,
    ) -> std::thread::Result<R> {
        Ok(f())
    }
}

// TODO: bikeshed
struct ReferencingShared<'a, P: Plugin> {
    audio_processor: Option<UnsafeCell<P::AudioProcessor<'a>>>,
    main_thread: UnsafeCell<P::MainThread<'a>>,
}

impl<'a, P: Plugin> ReferencingShared<'a, P> {
    fn new<'x>(
        // FIXME: why 'x: 'a is not accepted?
        host: HostMainThreadHandle<'x>,
        shared: &'a P::Shared<'x>,
    ) -> Result<Self, PluginError> {
        // TODO: safety
        let shared: &P::Shared<'a> =
            unsafe { &*(shared as *const P::Shared<'x> as *const () as *const P::Shared<'a>) };

        let host: HostMainThreadHandle<'a> = host.into_sublifetime();

        Ok(Self {
            main_thread: UnsafeCell::new(P::MainThread::new(host.into_sublifetime(), shared)?),
            audio_processor: None,
        })
    }

    fn activate<'x: 'a>(
        &mut self,
        host: HostAudioThreadHandle<'a>,
        shared: &'a P::Shared<'x>,
        audio_config: AudioConfiguration,
    ) -> Result<(), PluginError> {
        // TODO: safety
        let shared: &P::Shared<'a> =
            unsafe { &*(shared as *const P::Shared<'x> as *const () as *const P::Shared<'a>) };
        let processor =
            P::AudioProcessor::activate(host, self.main_thread.get_mut(), shared, audio_config)?;
        self.audio_processor = Some(UnsafeCell::new(processor));
        Ok::<_, PluginError>(())
    }
}

struct ReferencingSharedRef<P: Plugin>(PhantomData<P>);

impl<'s, P: Plugin> RefType<'s> for ReferencingSharedRef<P> {
    type Ref = ReferencingShared<'s, P>;
}

/// A wrapper around a `clack` plugin of a given type.
///
/// This wrapper allows access to a plugin's [`Shared`](Plugin::Shared),
/// [`MainThread`](Plugin::MainThread), and [`AudioProcessor`](Plugin::AudioProcessor) structs, while
/// also handling common FFI issues, such as error management and unwind safety.
///
/// The only way to access an instance of `PluginWrapper` is through the
/// [`handle`](PluginWrapper::handle) function.
pub struct PluginWrapper<'a, P: Plugin> {
    // audio_processor: Option<UnsafeCell<P::AudioProcessor<'a>>>,
    // main_thread: UnsafeCell<P::MainThread<'a>>,
    selfie: Selfie<'a, Box<P::Shared<'a>>, ReferencingSharedRef<P>>,
    // shared: Pin<Box<P::Shared<'a>>>,
    host: HostHandle<'a>,
}

impl<'a, P: Plugin> PluginWrapper<'a, P> {
    pub(crate) fn new(host: HostMainThreadHandle<'a>) -> Result<Self, PluginError> {
        let shared = Box::pin(P::Shared::new(host.shared())?);

        let selfie = Selfie::<'a, _, ReferencingSharedRef<P>>::try_new(
            shared,
            move |shared_ref: &'_ P::Shared<'a>| ReferencingShared::new(host, shared_ref),
        )
        .map_err(|e| e.error)?;

        Ok(Self {
            host: host.shared(),
            selfie,
        })
    }

    /// # Safety
    /// Caller must ensure this method is only called on main thread and has exclusivity
    pub(crate) unsafe fn activate(
        &mut self,
        audio_config: AudioConfiguration,
    ) -> Result<(), PluginWrapperError> {
        if self.is_active() {
            return Err(PluginWrapperError::ActivatedPlugin);
        }

        let shared = &*(self.shared() as *const _);
        let host = self.host;

        self.selfie.with_referential_mut(|pinned_self| {
            pinned_self.activate(host.as_audio_thread_unchecked(), shared, audio_config)
        })?;

        Ok(())
    }

    /// # Safety
    /// Caller must ensure this method is only called on main thread, and has exclusivity on it
    pub(crate) unsafe fn deactivate(&mut self) -> Result<(), PluginWrapperError> {
        // SAFETY: taking the audio processor does not move the whole struct
        self.selfie.with_referential_mut(|pinned_self| {
            let audio_processor = &mut pinned_self.audio_processor;

            match audio_processor.take() {
                None => Err(PluginWrapperError::DeactivatedPlugin),
                Some(audio_processor) => {
                    audio_processor
                        .into_inner()
                        .deactivate(pinned_self.main_thread.get_mut());

                    Ok(())
                }
            }
        })
    }

    /// Returns if the current plugin has been activated or not.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.selfie
            .with_referential(|s| s.audio_processor.is_some())
    }

    /// Returns a reference to a plugin's [`Shared`](Plugin::Shared) struct.
    ///
    /// This is always safe to call in any context, since the `Shared` struct is required to
    /// implement `Sync`.
    #[inline]
    pub fn shared(&self) -> &P::Shared<'a> {
        self.selfie.owned()
    }

    /// Returns a raw, non-null pointer to the plugin's [`MainThread`](Plugin::MainThread)
    /// struct.
    ///
    /// # Safety
    /// The caller must ensure this method is only called on the main thread.
    ///
    /// The pointer is safe to mutably dereference, as long as the caller ensures it is not being
    /// aliased, as per usual safety rules.
    #[inline]
    pub unsafe fn main_thread(&self) -> NonNull<P::MainThread<'_>> {
        self.selfie.with_referential(|s| {
            // SAFETY: pointer has been created from reference, it cannot be null.
            NonNull::new_unchecked(s.main_thread.get())
        })
    }

    /// Returns a raw, non-null pointer to the plugin's audio processor
    /// (i.e. [`Plugin`](PluginAudioProcessor)) struct.
    ///
    /// # Errors
    ///
    /// This method will return `PluginWrapperError::DeactivatedPlugin` if the plugin has not been
    /// activated before calling this method.
    ///
    /// This is an extra safety check which ensures that hosts correctly activated plugins before
    /// calling any audio-thread method.
    ///
    /// # Safety
    /// The caller must ensure this method is only called on the audio thread.
    ///
    /// The pointer is safe to mutably dereference, as long as the caller ensures it is not being
    /// aliased, as per usual safety rules.
    #[inline]
    pub unsafe fn audio_processor(
        &self,
    ) -> Result<NonNull<P::AudioProcessor<'_>>, PluginWrapperError> {
        self.selfie.with_referential(|s| {
            s.audio_processor
                .as_ref()
                // SAFETY: pointer has been created from reference, it cannot be null.
                .map(|p| NonNull::new_unchecked(p.get()))
                .ok_or(PluginWrapperError::DeactivatedPlugin)
        })
    }

    /// Provides a shared reference to a plugin wrapper of a given type, to the given handler
    /// closure.
    ///
    /// Besides providing a reference, this function does a few extra safety checks:
    ///
    /// * The given `clap_plugin` pointer is null-checked, as well as some other host-provided
    /// pointers;
    /// * The handler is wrapped in [`std::panic::catch_unwind`];
    /// * Any [`PluginWrapperError`] returned by the handler is caught.
    ///
    /// If any of the above safety check fails, an error message is logged (using the standard CLAP
    /// logging extension). If logging is unavailable or fails for any reason, the error message is
    /// written to `stderr` as a fallback.
    ///
    /// Note that some safety checks (e.g. the `clap_plugin` pointer null-checks) may result in the
    /// closure never being called, and an error being returned only. Users of this function must
    /// not rely on the completion of this closure for safety, and must handle this function
    /// returning `None` gracefully.
    ///
    /// If all goes well, the return value of the handler closure is forwarded and returned by this
    /// function.
    ///
    /// # Errors
    /// If any safety check failed, or any error or panic occurred inside the handler closure, this
    /// function returns `None`, and the error message is logged.
    ///
    /// # Safety
    ///
    /// The given plugin type `P` **must** be the correct type for the received pointer. Otherwise,
    /// incorrect casts will occur, which will lead to Undefined Behavior.
    ///
    /// The `plugin` pointer must also point to a valid instance of `clap_plugin`, as provided by
    /// the CLAP Host. While this function does a couple of simple safety checks, only a few common
    /// cases are actually covered (i.e. null checks), and those **must not** be relied upon: those
    /// checks only exist to help debugging faulty hosts.
    ///
    /// # Example
    ///
    /// This is the implementation of the [`on_main_thread`](PluginMainThread::on_main_thread)
    /// callback's C wrapper.
    ///
    /// This method is guaranteed by the CLAP specification to be only called on the main thread.
    ///
    /// ```
    /// use clap_sys::plugin::clap_plugin;
    /// use clack_plugin::plugin::{Plugin, PluginMainThread};
    /// use clack_plugin::extensions::wrapper::PluginWrapper;
    ///
    /// unsafe extern "C" fn on_main_thread<P: Plugin>(plugin: *const clap_plugin) {
    ///   PluginWrapper::<P>::handle(plugin, |p| {
    ///     p.main_thread().as_mut().on_main_thread();
    ///     Ok(())
    ///   });
    /// }
    /// ```
    pub unsafe fn handle<T, F>(plugin: *const clap_plugin, handler: F) -> Option<T>
    where
        F: FnOnce(&PluginWrapper<'a, P>) -> Result<T, PluginWrapperError>,
    {
        match Self::handle_panic(plugin, handler) {
            Ok(value) => Some(value),
            Err(e) => {
                logging::plugin_log::<P>(plugin, &e);

                None
            }
        }
    }

    /// # Safety
    /// The plugin pointer must be valid
    pub(crate) unsafe fn handle_plugin_mut<T, F>(
        plugin: *const clap_plugin,
        handler: F,
    ) -> Option<T>
    where
        F: FnOnce(&mut PluginWrapper<'a, P>) -> Result<T, PluginWrapperError>,
    {
        match Self::handle_panic_mut(plugin, handler) {
            Ok(value) => Some(value),
            Err(e) => {
                logging::plugin_log::<P>(plugin, &e);

                None
            }
        }
    }

    unsafe fn from_raw<'p>(raw: *const clap_plugin) -> Result<&'p Self, PluginWrapperError> {
        raw.as_ref()
            .ok_or(PluginWrapperError::NullPluginInstance)?
            .plugin_data
            .cast::<PluginBoxInner<'a, P>>()
            .as_ref()
            .ok_or(PluginWrapperError::NullPluginData)?
            .plugin_data
            .as_ref()
            .ok_or(PluginWrapperError::UninitializedPlugin)
    }

    unsafe fn from_raw_mut<'p>(
        raw: *const clap_plugin,
    ) -> Result<&'p mut Self, PluginWrapperError> {
        raw.as_ref()
            .ok_or(PluginWrapperError::NullPluginInstance)?
            .plugin_data
            .cast::<PluginBoxInner<'a, P>>()
            .as_mut()
            .ok_or(PluginWrapperError::NullPluginData)?
            .plugin_data
            .as_mut()
            .ok_or(PluginWrapperError::UninitializedPlugin)
    }

    unsafe fn handle_panic<T, F>(
        plugin: *const clap_plugin,
        handler: F,
    ) -> Result<T, PluginWrapperError>
    where
        F: FnOnce(&Self) -> Result<T, PluginWrapperError>,
    {
        let plugin = Self::from_raw(plugin)?;

        panic::catch_unwind(AssertUnwindSafe(|| handler(plugin)))
            .map_err(|_| PluginWrapperError::Panic)?
    }

    unsafe fn handle_panic_mut<T, F>(
        plugin: *const clap_plugin,
        handler: F,
    ) -> Result<T, PluginWrapperError>
    where
        F: FnOnce(&mut Self) -> Result<T, PluginWrapperError>,
    {
        let plugin = Self::from_raw_mut(plugin)?;

        panic::catch_unwind(AssertUnwindSafe(|| handler(plugin)))
            .map_err(|_| PluginWrapperError::Panic)?
    }
}

unsafe impl<'a, P: Plugin> Send for PluginWrapper<'a, P> {}
unsafe impl<'a, P: Plugin> Sync for PluginWrapper<'a, P> {}

/// Errors raised by a [`PluginWrapper`].
#[derive(Debug)]
pub enum PluginWrapperError {
    /// The `clap_plugin` raw pointer was null.
    NullPluginInstance,
    /// The `clap_plugin.plugin_data` raw pointer was null.
    NullPluginData,
    /// A unexpectedly null raw pointer was encountered.
    ///
    /// The given string may contain more information about which pointer was found to be null.
    NulPtr(&'static str),
    /// An invalid parameter value was encountered.
    ///
    /// The given string may contain more information about which parameter was found to be invalid.
    InvalidParameter(&'static str),
    /// The plugin was not properly initialized (i.e. `init` was not called or had failed).
    UninitializedPlugin,
    /// An attempt was made to call `activate` on an already activated plugin.
    ActivatedPlugin,
    /// An attempt was made to call an audio-thread function while the plugin was deactivated
    /// (e.g. without previously calling `activate`).
    DeactivatedPlugin,
    /// A function which requires the plugin to be deactivated was called while the plugin was still
    /// active.
    DeactivationRequiredForFunction(&'static str),
    /// The plugin panicked during a function call.
    Panic,
    /// A given [`PluginError`](PluginError) was raised during a function call.
    Plugin(PluginError),
    /// Bad UTF-8.
    StringEncoding(std::str::Utf8Error),
    /// Plugin returned a malformed C string.
    InvalidCString(std::ffi::NulError),
    /// A generic or custom error of a given severity.
    Any(clap_log_severity, Box<dyn Error>),
}

impl PluginWrapperError {
    /// Returns the severity of this error.
    ///
    /// This is mainly useful for logging.
    ///
    /// # Example
    ///
    /// ```
    /// use clap_sys::ext::log::CLAP_LOG_PLUGIN_MISBEHAVING;
    /// use clack_plugin::extensions::wrapper::PluginWrapperError;
    /// let error = PluginWrapperError::Panic;
    ///
    /// assert_eq!(error.severity(), CLAP_LOG_PLUGIN_MISBEHAVING);
    /// ```
    pub fn severity(&self) -> clap_log_severity {
        match self {
            PluginWrapperError::Plugin(_) => CLAP_LOG_ERROR,
            PluginWrapperError::Panic => CLAP_LOG_PLUGIN_MISBEHAVING,
            PluginWrapperError::Any(s, _) => *s,
            _ => CLAP_LOG_HOST_MISBEHAVING,
        }
    }

    /// Returns a closure that maps an error to a [`PluginWrapperError::Any`] error of a given
    /// severity.
    ///
    /// This is an useful helper method when paired with [`Result::map_err`].
    ///
    /// # Example
    /// ```
    /// use clap_sys::ext::log::CLAP_LOG_PLUGIN_MISBEHAVING;
    /// use clack_plugin::extensions::wrapper::PluginWrapperError;
    ///
    /// let x: Result<(), _> = Err(std::env::VarError::NotPresent); // Some random error type
    /// let clap_error = x.map_err(PluginWrapperError::with_severity(CLAP_LOG_PLUGIN_MISBEHAVING));
    ///
    /// assert_eq!(clap_error.unwrap_err().severity(), CLAP_LOG_PLUGIN_MISBEHAVING);
    /// ```
    #[inline]
    pub fn with_severity<E: 'static + Error>(
        severity: clap_log_severity,
    ) -> impl Fn(E) -> PluginWrapperError {
        move |e| PluginWrapperError::Any(severity, Box::new(e))
    }
}

impl From<PluginError> for PluginWrapperError {
    #[inline]
    fn from(e: PluginError) -> Self {
        PluginWrapperError::Plugin(e)
    }
}

impl Display for PluginWrapperError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginWrapperError::NullPluginInstance => {
                f.write_str("Plugin method was called with null clap_plugin pointer")
            }
            PluginWrapperError::NullPluginData => {
                f.write_str("Plugin method was called with null clap_plugin.plugin_data pointer")
            }
            PluginWrapperError::NulPtr(ptr_name) => {
                write!(f, "Plugin method was called with null {ptr_name} pointer")
            }
            PluginWrapperError::InvalidParameter(p) => {
                write!(f, "Received invalid parameter '{p}'")
            }
            PluginWrapperError::UninitializedPlugin => {
                f.write_str("Plugin was not properly initialized before use")
            }
            PluginWrapperError::ActivatedPlugin => f.write_str("Plugin was already activated"),
            PluginWrapperError::DeactivatedPlugin => {
                f.write_str("Plugin was not activated before calling a audio-thread method")
            }
            PluginWrapperError::DeactivationRequiredForFunction(function) => write!(
                f,
                "Host attempted to call '{function}' while plugin was still active"
            ),
            PluginWrapperError::StringEncoding(e) => {
                write!(
                    f,
                    "Encountered string containing invalid UTF-8 at position {}.",
                    e.valid_up_to()
                )
            }
            PluginWrapperError::InvalidCString(e) => {
                write!(
                    f,
                    "Encountered string containing a NUL byte at position {}.",
                    e.nul_position()
                )
            }
            PluginWrapperError::Plugin(e) => std::fmt::Display::fmt(&e, f),
            PluginWrapperError::Any(_, e) => std::fmt::Display::fmt(e, f),
            PluginWrapperError::Panic => f.write_str("Plugin panicked"),
        }
    }
}

impl Error for PluginWrapperError {}
