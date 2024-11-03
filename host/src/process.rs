//! Plugin audio processing types and related utilities.
//!
//! A plugin's audio processor can be in two states, either `started` or `stopped`, depending on
//! whether it is currently continuously processing audio, or has been put to sleep by the host,
//! respectively.
//!
//! These states are represented by the [`StartedPluginAudioProcessor`] and [`StoppedPluginAudioProcessor`]
//! types respectively, using a type-state pattern that only expose the methods that are valid for
//! the plugin's current state.
//!
//! Alternatively, users can also use the [`PluginAudioProcessor`] convenience type that internalizes
//! the state instead, and allows it to change and be checked at runtime.

#![deny(missing_docs)]

use crate::host::HostHandlers;
use crate::plugin::{PluginAudioProcessorHandle, PluginInstanceError, PluginSharedHandle};
use crate::prelude::{AudioBuffers, PluginInstance};
use crate::process::PluginAudioProcessor::*;
use clack_common::events::event_types::TransportEvent;
use clack_common::events::io::{InputEvents, OutputEvents};
use clap_sys::process::clap_process;
use std::cell::UnsafeCell;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::plugin::instance::PluginInstanceInner;
pub use clack_common::process::*;

#[allow(missing_docs)] // TODO: doc this
pub mod audio_buffers;

/// A handle to a plugin's audio processor that can be in either its `started` or `stopped` state.
///
/// This is a convenience type that can be used where the type-states [`StartedPluginAudioProcessor`] and
/// [`StoppedPluginAudioProcessor`] are not very ergonomic, or to perform operations that are common
/// to both states.
///
/// Note that in order to maintain thread-safety, this type must be either dropped or stopped and
/// passed to either [`PluginInstance::deactivate`] or [`PluginInstance::deactivate_with`], *before*
/// the [`PluginInstance`] itself is dropped. If this is not done, the plugin instance will never
/// be deactivated or destroyed, and will leak all its associated resources.
pub enum PluginAudioProcessor<H: HostHandlers> {
    /// The audio processor is in it's `started` state.
    Started(StartedPluginAudioProcessor<H>),
    /// The audio processor is in it's `stopped` state.
    Stopped(StoppedPluginAudioProcessor<H>),
}

impl<H: HostHandlers> PluginAudioProcessor<H> {
    /// Resets the plugin's audio processing state.
    ///
    /// This clears all the plugin's internal buffers, kills all voices, and resets all processing
    /// state such as envelopes, LFOs, oscillators, filters, etc.
    ///
    /// Calling this method allows the `steady_time` parameter passed to [`process`](StartedPluginAudioProcessor::process)
    /// to jump backwards.
    #[inline]
    pub fn reset(&mut self) {
        match self {
            Started(s) => s.reset(),
            Stopped(s) => s.reset(),
        }
    }

    /// Accesses the [`SharedHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`SharedHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// Unlike [`access_handler`](self.access_handler) and
    /// [`access_handler_mut`](self.access_handler_mut), there is no way to obtain a mutable
    /// reference to the [`SharedHandler`], as it may be concurrently accessed by other threads.
    ///
    /// [`SharedHandler`]: crate::prelude::SharedHandler
    #[inline]
    pub fn access_shared_handler<'s, R>(
        &'s self,
        access: impl for<'a> FnOnce(&'s <H as HostHandlers>::Shared<'a>) -> R,
    ) -> R {
        match self {
            Started(s) => s.access_shared_handler(access),
            Stopped(s) => s.access_shared_handler(access),
        }
    }

    /// Accesses the [`AudioProcessorHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`AudioProcessorHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// See the [`access_handler_mut`](self.access_handler_mut) method to receive a mutable
    /// reference to the [`AudioProcessorHandler`] instead.
    ///
    /// [`AudioProcessorHandler`]: crate::prelude::AudioProcessorHandler
    #[inline]
    pub fn access_handler<'s, R>(
        &'s self,
        access: impl for<'a> FnOnce(&'s <H as HostHandlers>::AudioProcessor<'a>) -> R,
    ) -> R {
        match self {
            Started(s) => s.access_handler(access),
            Stopped(s) => s.access_handler(access),
        }
    }

    /// Accesses the [`AudioProcessorHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`AudioProcessorHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// See the [`access_handler`](self.access_handler) method to receive a shared
    /// reference to the [`AudioProcessorHandler`] instead.
    ///
    /// [`AudioProcessorHandler`]: crate::prelude::AudioProcessorHandler
    #[inline]
    pub fn access_handler_mut<'s, R>(
        &'s mut self,
        access: impl for<'a> FnOnce(&'s mut <H as HostHandlers>::AudioProcessor<'a>) -> R,
    ) -> R {
        match self {
            Started(s) => s.access_handler_mut(access),
            Stopped(s) => s.access_handler_mut(access),
        }
    }

    /// Returns this plugin instance's audio processor handle.
    #[inline]
    pub fn plugin_handle(&mut self) -> PluginAudioProcessorHandle {
        match self {
            Started(s) => s.plugin_handle(),
            Stopped(s) => s.plugin_handle(),
        }
    }

    /// Returns this plugin instance's shared handle.
    #[inline]
    pub fn shared_plugin_handle(&self) -> PluginSharedHandle {
        match self {
            Started(s) => s.shared_plugin_handle(),
            Stopped(s) => s.shared_plugin_handle(),
        }
    }

    /// Returns `true` if this audio processor was created from the given plugin instance.
    #[inline]
    pub fn matches(&self, instance: &PluginInstance<H>) -> bool {
        match &self {
            Started(s) => s.matches(instance),
            Stopped(s) => s.matches(instance),
        }
    }

    /// Returns `true` if this audio processor is its `started` state, `false` otherwise.
    #[inline]
    pub fn is_started(&self) -> bool {
        match self {
            Stopped(_) => false,
            Started(_) => true,
        }
    }

    /// Returns this audio processor as a shared reference to its [`StartedPluginAudioProcessor`]
    /// state, if it is in the `started` state.
    ///
    /// # Errors
    ///
    /// This returns a [`PluginInstanceError::ProcessingStopped`] error if the audio processor is
    /// in the `stopped` state.
    #[inline]
    pub fn as_started(&self) -> Result<&StartedPluginAudioProcessor<H>, PluginInstanceError> {
        match self {
            Started(s) => Ok(s),
            Stopped(_) => Err(PluginInstanceError::ProcessingStopped),
        }
    }

    /// Returns this audio processor as a mutable reference to its [`StartedPluginAudioProcessor`]
    /// state, if it is in the `started` state.
    ///
    /// # Errors
    ///
    /// This returns a [`PluginInstanceError::ProcessingStopped`] error if the audio processor is
    /// in the `stopped` state.
    #[inline]
    pub fn as_started_mut(
        &mut self,
    ) -> Result<&mut StartedPluginAudioProcessor<H>, PluginInstanceError> {
        match self {
            Started(s) => Ok(s),
            Stopped(_) => Err(PluginInstanceError::ProcessingStopped),
        }
    }

    /// Returns this audio processor as a shared reference to its [`StoppedPluginAudioProcessor`]
    /// state, if it is in the `stopped` state.
    ///
    /// # Errors
    ///
    /// This returns a [`PluginInstanceError::ProcessingStarted`] error if the audio processor is
    /// in the `started` state.
    #[inline]
    pub fn as_stopped(&self) -> Result<&StoppedPluginAudioProcessor<H>, PluginInstanceError> {
        match self {
            Stopped(s) => Ok(s),
            Started(_) => Err(PluginInstanceError::ProcessingStarted),
        }
    }

    /// Returns this audio processor as a mutable reference to its [`StoppedPluginAudioProcessor`]
    /// state, if it is in the `stopped` state.
    ///
    /// # Errors
    ///
    /// This returns a [`PluginInstanceError::ProcessingStarted`] error if the audio processor is
    /// in the `started` state.
    #[inline]
    pub fn as_stopped_mut(
        &mut self,
    ) -> Result<&mut StoppedPluginAudioProcessor<H>, PluginInstanceError> {
        match self {
            Stopped(s) => Ok(s),
            Started(_) => Err(PluginInstanceError::ProcessingStarted),
        }
    }

    /// Starts this audio processor, and returns a mutable reference to its
    /// [`StartedPluginAudioProcessor`] state.
    ///
    /// # Errors
    ///
    /// If the start operation failed, this returns a [`PluginInstanceError::StartProcessingFailed`] error.
    /// If the audio processor was already started, this returns a
    /// [`PluginInstanceError::ProcessingStarted`] error.
    #[inline]
    pub fn start_processing(
        &mut self,
    ) -> Result<&mut StartedPluginAudioProcessor<H>, PluginInstanceError> {
        let inner = match self {
            Started(_) => return Err(PluginInstanceError::ProcessingStarted),
            Stopped(a) => a.inner.clone(),
        };

        let Ok(started) = StoppedPluginAudioProcessor::new(inner).start_processing() else {
            return Err(PluginInstanceError::StartProcessingFailed);
        };

        *self = Started(started);

        Ok(match self {
            Started(s) => s,
            _ => unreachable!(),
        })
    }

    /// Starts this audio processor, and returns a mutable reference to its
    /// [`StoppedPluginAudioProcessor`] state.
    ///
    /// # Errors
    ///
    /// If the audio processor was already stopped, this returns a
    /// [`PluginInstanceError::ProcessingStopped`] error.
    #[inline]
    pub fn stop_processing(
        &mut self,
    ) -> Result<&mut StoppedPluginAudioProcessor<H>, PluginInstanceError> {
        let inner = match self {
            Stopped(_) => return Err(PluginInstanceError::ProcessingStopped),
            Started(a) => a.inner.clone(),
        };

        let stopped = StartedPluginAudioProcessor::new(inner).stop_processing();

        *self = Stopped(stopped);

        Ok(match self {
            Stopped(s) => s,
            _ => unreachable!(),
        })
    }

    /// Starts this audio processor, if it is not already started, and returns a mutable reference
    /// to its [`StartedPluginAudioProcessor`] state.
    ///
    /// If its already started, this does nothing and returns the mutable reference directly.
    ///
    /// # Errors
    ///
    /// If this audio processor was in its `stopped` state and the start operation failed, this
    /// returns a [`PluginInstanceError::StartProcessingFailed`] error.
    #[inline]
    pub fn ensure_processing_started(
        &mut self,
    ) -> Result<&mut StartedPluginAudioProcessor<H>, PluginInstanceError> {
        match self {
            Started(s) => Ok(s),
            _ => self.start_processing(),
        }
    }

    /// Stops this audio processor, if it is not already stopped, and returns a mutable reference
    /// to its [`StoppedPluginAudioProcessor`] state.
    ///
    /// If its already stopped, this does nothing and returns the mutable reference directly.
    ///
    /// This operation is infallible.
    #[inline]
    pub fn ensure_processing_stopped(&mut self) -> &mut StoppedPluginAudioProcessor<H> {
        let inner = match self {
            Stopped(s) => return s,
            Started(a) => a.inner.clone(),
        };

        let stopped = StartedPluginAudioProcessor::new(inner).stop_processing();

        *self = Stopped(stopped);

        match self {
            Stopped(s) => s,
            _ => unreachable!(),
        }
    }

    /// Starts this audio processor, if it is not already started, and returns its
    /// [`StartedPluginAudioProcessor`] state, consuming the [`PluginAudioProcessor`] instance in
    /// the process.
    ///
    /// If it is already started, this does nothing and returns the [`StartedPluginAudioProcessor`] state directly.
    ///
    /// # Errors
    ///
    /// If this audio processor was in its `stopped` state and the start operation failed, this
    /// returns a [`ProcessingStartError`], from which the [`StoppedPluginAudioProcessor`] can be
    /// recovered.
    #[inline]
    pub fn into_started(self) -> Result<StartedPluginAudioProcessor<H>, ProcessingStartError<H>> {
        match self {
            Started(s) => Ok(s),
            Stopped(s) => s.start_processing(),
        }
    }

    /// Stops this audio processor, if it is not already stopped, and returns its
    /// [`StoppedPluginAudioProcessor`] state, consuming the [`PluginAudioProcessor`] instance in
    /// the process.
    ///
    /// If it is already stopped, this does nothing and returns the [`StoppedPluginAudioProcessor`] state directly.
    ///
    /// This operation is infallible.
    #[inline]
    pub fn into_stopped(self) -> StoppedPluginAudioProcessor<H> {
        match self {
            Started(s) => s.stop_processing(),
            Stopped(s) => s,
        }
    }
}

impl<H: HostHandlers> From<StartedPluginAudioProcessor<H>> for PluginAudioProcessor<H> {
    #[inline]
    fn from(p: StartedPluginAudioProcessor<H>) -> Self {
        Started(p)
    }
}

impl<H: HostHandlers> From<StoppedPluginAudioProcessor<H>> for PluginAudioProcessor<H> {
    #[inline]
    fn from(p: StoppedPluginAudioProcessor<H>) -> Self {
        Stopped(p)
    }
}

/// A handle to a plugin's audio processor that is in the `started` state.
///
/// A host can call [`process`] or [`stop_processing`] on a plugin which audio processor is in
/// this state.
///
/// This type is generic over the [`HostHandlers`] type that was used to create the plugin instance.
/// The [shared] and [audio processor] handlers can be accessed with the
/// [`access_shared_handler`](Self::access_shared_handler),
/// [`access_handler`](Self::access_handler) and [`access_handler_mut`](Self::access_handler_mut)
/// methods.
///
/// Note that in order to maintain thread-safety, this type must be either dropped or stopped and
/// passed to either [`PluginInstance::deactivate`] or [`PluginInstance::deactivate_with`], *before*
/// the [`PluginInstance`] itself is dropped. If this is not done, the plugin instance will never
/// be deactivated or destroyed, and will leak all its associated resources.
///
/// [`process`]: Self::process
/// [`stop_processing`]: Self::stop_processing
/// [shared]: crate::prelude::SharedHandler
/// [audio processor]: crate::prelude::AudioProcessorHandler
/// [`destroy`](PluginInstance::deactivate)
pub struct StartedPluginAudioProcessor<H: HostHandlers> {
    inner: Arc<PluginInstanceInner<H>>,
    _no_sync: PhantomData<UnsafeCell<()>>,
}

impl<H: HostHandlers> StartedPluginAudioProcessor<H> {
    #[inline]
    fn new(inner: Arc<PluginInstanceInner<H>>) -> Self {
        Self {
            inner,
            _no_sync: PhantomData,
        }
    }

    /// Process a chunk of audio frames and events.
    ///
    /// This plugin function requires the following arguments:
    /// * `audio_inputs`: The [`AudioBuffers`] the plugin is going to read audio frames from.
    ///   Can be [`AudioBuffers::empty`] if the plugin takes no audio input at all.
    /// * `audio_output`: The [`AudioBuffers`] the plugin is going to read audio frames from.
    ///   Can be [`AudioBuffers::empty`] if the plugin produces no audio output at all.
    /// * `input_events`: The [`InputEvents`] list the plugin is going to receive events from.
    ///   Can be [`InputEvents::empty`] if the plugin doesn't need to receive any events.
    /// * `output_events`: The [`OutputEvents`] buffer the plugin is going to write the events it
    ///   produces.
    ///   Can be [`OutputEvents::void`] to ignore any events the plugin produces.
    ///
    /// Additionally, the following optional arguments can also be given:
    ///
    /// * `steady_time`: A steady sample time counter.
    ///   This can be used to calculate the sleep duration between two process calls.
    ///   This value may be specific to this plugin instance and have no relation to what
    ///   other plugin instances may receive.
    ///
    ///   The only requirement is that this value must be increased by at least the frame count
    ///   of the audio buffers (see [`AudioBuffers::min_available_frames_with`]) for the next
    ///   call to `process`.
    ///
    ///   This value can never decrease between two calls to `process`, unless [`reset`]
    ///   is called, or if it was increased beyond [`u64::MAX`] and it wrapped around.
    ///
    ///   This can be set to `None` if not available.
    ///
    /// * `transport`: Transport information, as of sample `0`. See the [`TransportEvent`]
    ///   documentation for more details about the available transport information.
    ///
    ///   This can be `None` if no transport is available, i.e. if the host is free-running.
    ///
    /// Once processing is complete, the function returns a [`ProcessStatus`] to inform the host
    /// whether the plugin can be put to sleep or not. See the [`ProcessStatus`] documentation
    /// for more information.
    ///
    /// # Errors
    ///
    /// This function can return [`PluginInstanceError::NullProcessFunction`] if the plugin
    /// implementation did not provide a valid underlying `process` function pointer.
    ///
    /// This can also return [`PluginInstanceError::ProcessingFailed`] if the `process` function
    /// failed for any reason.
    ///
    /// [`reset`]: Self::reset
    pub fn process(
        &mut self,
        audio_inputs: &AudioBuffers,
        audio_outputs: &AudioBuffers,
        input_events: &InputEvents,
        output_events: &mut OutputEvents,
        steady_time: Option<u64>,
        transport: Option<&TransportEvent>,
    ) -> Result<ProcessStatus, PluginInstanceError> {
        let frames_count = audio_inputs.min_available_frames_with(audio_outputs);

        let process = clap_process {
            frames_count,

            in_events: input_events.as_raw(),
            out_events: output_events.as_raw_mut(),

            audio_inputs: audio_inputs.as_raw_buffers().cast(),
            audio_outputs: audio_outputs.as_raw_buffers().cast(),
            audio_inputs_count: audio_inputs.port_count() as u32,
            audio_outputs_count: audio_outputs.port_count() as u32,

            steady_time: match steady_time {
                None => -1,
                // This is a wrapping conversion from u64 to i64.
                // The wrapping allows smooth operation from the plugin if steady_time does actually overflow an i64.
                Some(steady_time) => (steady_time & i64::MAX as u64) as i64,
            },
            transport: match transport {
                None => core::ptr::null(),
                Some(e) => e.as_raw(),
            },
        };

        let instance = self.inner.raw_instance();

        let process_fn = instance
            .process
            .ok_or(PluginInstanceError::NullProcessFunction)?;

        // SAFETY: this type ensures the function pointer is valid
        let status = unsafe { process_fn(instance, &process) };

        match ProcessStatus::from_raw(status) {
            None | Some(Err(())) => Err(PluginInstanceError::ProcessingFailed),
            Some(Ok(status)) => Ok(status),
        }
    }

    /// Resets the plugin's audio processing state.
    ///
    /// This clears all the plugin's internal buffers, kills all voices, and resets all processing
    /// state such as envelopes, LFOs, oscillators, filters, etc.
    ///
    /// Calling this method allows the `steady_time` parameter passed to [`process`](Self::process)
    /// to jump backwards.
    #[inline]
    pub fn reset(&mut self) {
        // SAFETY: This type ensures this can only be called in the main thread.
        unsafe { self.inner.reset() }
    }

    /// Sends the plugin to sleep, implying the next [`process`](Self::process) call will not be
    /// continuous with the last one.
    ///
    /// This method returns an audio processor in the [stopped](StoppedPluginAudioProcessor) state.
    ///
    /// This operation is infallible.
    #[inline]
    pub fn stop_processing(self) -> StoppedPluginAudioProcessor<H> {
        let inner = self.inner;
        // SAFETY: this is called on the audio thread
        unsafe { inner.stop_processing() };

        StoppedPluginAudioProcessor {
            inner,
            _no_sync: PhantomData,
        }
    }

    /// Accesses the [`SharedHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`SharedHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// Unlike [`access_handler`](self.access_handler) and
    /// [`access_handler_mut`](self.access_handler_mut), there is no way to obtain a mutable
    /// reference to the [`SharedHandler`], as it may be concurrently accessed by other threads.
    ///
    /// [`SharedHandler`]: crate::prelude::SharedHandler
    #[inline]
    pub fn access_shared_handler<'s, R>(
        &'s self,
        access: impl for<'a> FnOnce(&'s <H as HostHandlers>::Shared<'a>) -> R,
    ) -> R {
        access(self.inner.wrapper().shared())
    }

    /// Accesses the [`AudioProcessorHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`AudioProcessorHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// See the [`access_handler_mut`](self.access_handler_mut) method to receive a mutable
    /// reference to the [`AudioProcessorHandler`] instead.
    ///
    /// [`AudioProcessorHandler`]: crate::prelude::AudioProcessorHandler
    #[inline]
    pub fn access_handler<'s, R>(
        &'s self,
        access: impl for<'a> FnOnce(&'s <H as HostHandlers>::AudioProcessor<'a>) -> R,
    ) -> R {
        // SAFETY: we take &mut self, the only reference to the wrapper on the audio thread,
        // therefore we can guarantee there are other references anywhere
        // PANIC: This struct exists, therefore we are guaranteed the plugin is active
        unsafe { access(self.inner.wrapper().audio_processor().unwrap().as_ref()) }
    }

    /// Accesses the [`AudioProcessorHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`AudioProcessorHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// See the [`access_handler`](self.access_handler) method to receive a shared
    /// reference to the [`AudioProcessorHandler`] instead.
    ///
    /// [`AudioProcessorHandler`]: crate::prelude::AudioProcessorHandler
    #[inline]
    pub fn access_handler_mut<'s, R>(
        &'s mut self,
        access: impl for<'a> FnOnce(&'s mut <H as HostHandlers>::AudioProcessor<'a>) -> R,
    ) -> R {
        // SAFETY: we take &self, the only reference to the wrapper on the audio thread, therefore
        // we can guarantee there are no mutable references anywhere
        // PANIC: This struct exists, therefore we are guaranteed the plugin is active
        unsafe { access(self.inner.wrapper().audio_processor().unwrap().as_mut()) }
    }
    /// Returns this plugin instance's audio processor handle.
    #[inline]
    pub fn plugin_handle(&mut self) -> PluginAudioProcessorHandle {
        PluginAudioProcessorHandle::new(self.inner.raw_instance().into())
    }

    /// Returns this plugin instance's shared handle.
    #[inline]
    pub fn shared_plugin_handle(&self) -> PluginSharedHandle {
        self.inner.plugin_shared()
    }

    /// Returns `true` if this audio processor was created from the given plugin instance.
    #[inline]
    pub fn matches(&self, instance: &PluginInstance<H>) -> bool {
        Arc::ptr_eq(&self.inner, &instance.inner)
    }
}

/// A handle to a plugin's audio processor that is in the `stopped` state.
///
/// This is the default state the plugin's audio processor will be in after calling [`activate`].
///
/// A host needs to call [`start_processing`] before it can call the [`process`] method to process
/// audio and events.
///
/// This type is generic over the [`HostHandlers`] type that was used to create the plugin instance.
/// The [shared] and [audio processor] handlers can be accessed with the
/// [`access_shared_handler`](Self::access_shared_handler),
/// [`access_handler`](Self::access_handler) and [`access_handler_mut`](Self::access_handler_mut)
/// methods.
///
/// Note that in order to maintain thread-safety, this type must be either dropped or stopped and
/// passed to either [`PluginInstance::deactivate`] or [`PluginInstance::deactivate_with`], *before*
/// the [`PluginInstance`] itself is dropped. If this is not done, the plugin instance will never
/// be deactivated or destroyed, and will leak all its associated resources.
///
/// [`activate`]: PluginInstance::activate
/// [`process`]: StartedPluginAudioProcessor::process
/// [`start_processing`]: Self::start_processing
/// [shared]: crate::prelude::SharedHandler
/// [audio processor]: crate::prelude::AudioProcessorHandler
pub struct StoppedPluginAudioProcessor<H: HostHandlers> {
    pub(crate) inner: Arc<PluginInstanceInner<H>>,
    _no_sync: PhantomData<UnsafeCell<()>>,
}

impl<H: HostHandlers> StoppedPluginAudioProcessor<H> {
    #[inline]
    pub(crate) fn new(inner: Arc<PluginInstanceInner<H>>) -> Self {
        Self {
            inner,
            _no_sync: PhantomData,
        }
    }

    /// Resets the plugin's audio processing state.
    ///
    /// This clears all the plugin's internal buffers, kills all voices, and resets all processing
    /// state such as envelopes, LFOs, oscillators, filters, etc.
    ///
    /// Calling this method allows the `steady_time` parameter passed to [`process`](StartedPluginAudioProcessor::process)
    /// to jump backwards.
    #[inline]
    pub fn reset(&mut self) {
        // SAFETY: This type ensures this can only be called in the main thread.
        unsafe { self.inner.reset() }
    }

    /// Indicates to the plugin that continuous processing is about to start.
    ///
    /// Calling this is required in order to be able to call the [`process`] method to process
    /// audio and events.
    ///
    /// If this succeeds, this returns an audio processor in the [started](StartedPluginAudioProcessor)
    /// state.
    ///
    /// # Errors
    ///
    /// This method can fail if the underlying plugin's implementation fails for any reason.
    /// In this case, a [`ProcessingStartError`] is returned, from which the stopped audio processor
    /// can be recovered.
    ///
    /// [`process`]: StartedPluginAudioProcessor::process
    #[inline]
    pub fn start_processing(
        self,
    ) -> Result<StartedPluginAudioProcessor<H>, ProcessingStartError<H>> {
        // SAFETY: this is called on the audio thread
        match unsafe { self.inner.start_processing() } {
            Ok(()) => Ok(StartedPluginAudioProcessor {
                inner: self.inner,
                _no_sync: PhantomData,
            }),
            Err(_) => Err(ProcessingStartError { processor: self }),
        }
    }

    /// Accesses the [`SharedHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`SharedHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// Unlike [`access_handler`](self.access_handler) and
    /// [`access_handler_mut`](self.access_handler_mut), there is no way to obtain a mutable
    /// reference to the [`SharedHandler`], as it may be concurrently accessed by other threads.
    ///
    /// [`SharedHandler`]: crate::prelude::SharedHandler
    #[inline]
    pub fn access_shared_handler<'s, R>(
        &'s self,
        access: impl for<'a> FnOnce(&'s <H as HostHandlers>::Shared<'a>) -> R,
    ) -> R {
        access(self.inner.wrapper().shared())
    }

    /// Accesses the [`AudioProcessorHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`AudioProcessorHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// See the [`access_handler_mut`](self.access_handler_mut) method to receive a mutable
    /// reference to the [`AudioProcessorHandler`] instead.
    ///
    /// [`AudioProcessorHandler`]: crate::prelude::AudioProcessorHandler
    #[inline]
    pub fn access_handler<'s, R>(
        &'s self,
        access: impl for<'a> FnOnce(&'s <H as HostHandlers>::AudioProcessor<'a>) -> R,
    ) -> R {
        // SAFETY: we take &mut self, the only reference to the wrapper on the audio thread,
        // therefore we can guarantee there are other references anywhere
        // PANIC: This struct exists, therefore we are guaranteed the plugin is active
        unsafe { access(self.inner.wrapper().audio_processor().unwrap().as_ref()) }
    }

    /// Accesses the [`AudioProcessorHandler`] for this instance, using the provided closure.
    ///
    /// This function returns the return value of the provided closure directly.
    ///
    /// Note that the lifetime of the [`AudioProcessorHandler`] cannot be statically known, as it is bound
    /// to the plugin instance itself.
    ///
    /// See the [`access_handler`](self.access_handler) method to receive a shared
    /// reference to the [`AudioProcessorHandler`] instead.
    ///
    /// [`AudioProcessorHandler`]: crate::prelude::AudioProcessorHandler
    #[inline]
    pub fn access_handler_mut<'s, R>(
        &'s mut self,
        access: impl for<'a> FnOnce(&'s mut <H as HostHandlers>::AudioProcessor<'a>) -> R,
    ) -> R {
        // SAFETY: we take &self, the only reference to the wrapper on the audio thread, therefore
        // we can guarantee there are no mutable references anywhere
        // PANIC: This struct exists, therefore we are guaranteed the plugin is active
        unsafe { access(self.inner.wrapper().audio_processor().unwrap().as_mut()) }
    }

    /// Returns this plugin instance's audio processor handle.
    #[inline]
    pub fn plugin_handle(&mut self) -> PluginAudioProcessorHandle {
        PluginAudioProcessorHandle::new(self.inner.raw_instance().into())
    }

    /// Returns this plugin instance's shared handle.
    #[inline]
    pub fn shared_plugin_handle(&self) -> PluginSharedHandle {
        self.inner.plugin_shared()
    }

    /// Returns `true` if this audio processor was created from the given plugin instance.
    #[inline]
    pub fn matches(&self, instance: &PluginInstance<H>) -> bool {
        Arc::ptr_eq(&self.inner, &instance.inner)
    }
}

/// An error that occurred when a plugin instance couldn't start processing.
///
/// The [`StoppedPluginAudioProcessor`] can be recovered using the
/// [`into_stopped_processor`](Self::into_stopped_processor) method.
pub struct ProcessingStartError<H: HostHandlers> {
    processor: StoppedPluginAudioProcessor<H>,
}

impl<H: HostHandlers> ProcessingStartError<H> {
    /// Recovers the plugin instance's [`StoppedPluginAudioProcessor`] that failed to start.
    #[inline]
    pub fn into_stopped_processor(self) -> StoppedPluginAudioProcessor<H> {
        self.processor
    }
}

impl<H: HostHandlers> Debug for ProcessingStartError<H> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("Failed to start plugin processing")
    }
}

impl<H: HostHandlers> Display for ProcessingStartError<H> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("Failed to start plugin processing")
    }
}

impl<H: HostHandlers> Error for ProcessingStartError<H> {}

#[cfg(test)]
mod test {
    extern crate static_assertions as sa;
    use super::*;

    sa::assert_not_impl_any!(StartedPluginAudioProcessor<()>: Sync);
    sa::assert_not_impl_any!(StoppedPluginAudioProcessor<()>: Sync);
}
