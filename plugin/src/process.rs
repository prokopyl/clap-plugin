//! Types exposing data and metadata to be used by plugins during audio processing.
//!
//! All of those types are exclusively used in the [`Plugin::process`](crate::plugin::PluginAudioProcessor::process)
//! method. See the [`Plugin`](crate::plugin::PluginAudioProcessor) trait documentation for examples on how these types interact.

use crate::internal_utils::slice_from_external_parts;
use clack_common::events::event_types::TransportEvent;
use clack_common::events::io::{InputEvents, OutputEvents};
pub use clack_common::process::*;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::process::clap_process;
use std::ops::RangeBounds;

pub mod audio;
use audio::*;

/// Metadata about the current process call.
///
/// This exposes [transport information](Process::transport) (in the form of a [`TransportEvent`]), as well as a
/// [steady sample time counter](Process::steady_time).
///
#[derive(Copy, Clone)]
pub struct Process<'a> {
    /// Transport information at sample 0.
    ///
    /// If this is set to [`None`], then this means the plugin is running is a free-running host,
    /// and no transport events will be provided.
    pub transport: Option<&'a TransportEvent>,
    /// A steady sample time counter.
    ///
    /// This field can be used to calculate the sleep duration between two process calls.
    /// This value may be specific to this plugin instance and have no relation to what
    /// other plugin instances may receive.
    ///
    /// If no steady sample time counter is available from the host, this is set to [`None`].
    ///
    /// Note that this counter's maximum value is actually [`i64::MAX`], due to how it is
    /// implemented in the CLAP specification.
    pub steady_time: Option<u64>,
}

impl<'a> Process<'a> {
    /// # Safety
    ///
    /// The user must ensure the given process struct is fully valid, and for the lifetime `'a`.
    #[inline]
    pub(crate) unsafe fn from_raw(raw: *const clap_process) -> Process<'a> {
        let transport = (*raw).transport;
        let steady_time = (*raw).steady_time;

        Self {
            steady_time: if steady_time < 0 {
                None
            } else {
                Some(steady_time as u64)
            },
            transport: if transport.is_null() {
                None
            } else {
                Some(TransportEvent::from_raw_ref(&*transport))
            },
        }
    }
}

/// Input and output events that occurred during this processing block.
pub struct Events<'a> {
    /// The input event buffer, for the plugin to read events from.
    pub input: &'a InputEvents<'a>,
    /// The output event buffer, for the plugin to push events into.
    pub output: &'a mut OutputEvents<'a>,
}

impl Events<'_> {
    /// # Safety
    ///
    /// The user must ensure the given process struct is fully valid, and for the lifetime `'a`.
    pub(crate) unsafe fn from_raw(process: &clap_process) -> Self {
        Self {
            input: InputEvents::from_raw(&*process.in_events),
            output: OutputEvents::from_raw_mut(&mut *(process.out_events as *mut _)),
        }
    }
}

/// Input and output audio buffers to processed by the plugin.
///
/// Audio data in CLAP follow the following tree structure:
///
/// * Plugins may have an arbitrary amount of input and output ports;
/// * Each port can hold either 32-bit, or 64-bit floating-point sample data;
/// * Port sample data is split in multiple channels (1 for mono, 2 for stereo, etc.);
/// * Each channel is a raw buffer (i.e. slice) of either [`f32`] or [`f64`] samples.
///
/// This structure applies both to inputs and outputs: the [`Audio`] struct allows to retrieve
/// input and output [`Port`]s separately, but they can also be accessed together as
/// Input/Output [`PortPair`]s. This allows for the common use-case of borrowing both an input
/// and its matching output for processing, while also being safe to hosts using the same buffer for
/// both.
///
/// For each port type (input, output, or paired), ports can be accessed either individually with
/// an index, or all at once with an iterator. For instance, [`Port`]s can be accessed either
/// one-at-a-time with [`Audio::input_port`], or with an iterator from [`Audio::input_ports`]. A
/// [`Audio::input_port_count`] method is also available. The same methods are available for
/// the output [`Port`]s and [`PortPair`]s.
///
/// Note that because ports can individually hold either 32-bit or 64-bit sample data, an extra
/// sample type detection step is necessary before the port's channels themselves can be accessed.
/// This is done through the [`channels`](Port::channels) methods on each port type, and
/// returns a [`SampleType`] enum indicating whether the port's buffers hold 32-bit or 64-bit samples.
///
/// # Example
///
/// The following example implements a gain plugin that amplifies every input channel by `2`, and
/// writes the result to a matching output channel.
///
/// ```
/// use clack_plugin::prelude::*;
///
/// pub fn process(audio: Audio) -> Result<ProcessStatus, PluginError> {
///     for mut port_pair in audio {
///         // For this example, we'll only care about 32-bit sample data.
///         let Some(channel_pairs) = port_pair.channels()?.to_f32() else { continue };
///
///         for channel_pair in channel_pairs {
///             match channel_pair {
///                 // If this input has no matching output, we simply do nothing with it.
///                 ChannelPair::InputOnly(_) => {}
///                 // If this output has no matching input, we fill it with silence.
///                 ChannelPair::OutputOnly(buf) => buf.fill(0.0),
///                 // If this is a separate pair of I/O buffers,
///                 // we can copy the input to the outputs while multiplying
///                 ChannelPair::InputOutput(input, output) => {
///                     for (input, output) in input.iter().zip(output) {
///                         output.set(input.get() * 2.0)
///                     }
///                 }
///                 // If the host sent us a single buffer to be processed in-place
///                 // (i.e. input and output buffers point to the same location),
///                 // then we can do the processing in-place directly.
///                 ChannelPair::InPlace(buf) => {
///                     for sample in buf {
///                         sample.set(sample.get() * 2.0)
///                     }
///                 }
///             }
///         }
///     }
///
///     Ok(ProcessStatus::Continue)
/// }
/// ```
///
/// The following example shows how to gather different inputs and outputs simultaneously, and here
/// uses it to simply swap the left and right stereo channels.
///
/// ```
/// use clack_plugin::prelude::*;
///
/// pub fn process(audio: Audio) -> Result<ProcessStatus, PluginError> {
///     for mut port_pair in audio {
///         // For this example, we'll only care about 32-bit sample data.
///         let Some(mut channel_pairs) = port_pair.channels()?.to_f32() else { continue; };
///
///         // Buffers to hold pointers to the left and right channels
///         let mut input_channels: [Option<&AudioBuffer<f32>>; 2] = [None, None];
///         let mut output_channels: [Option<&AudioBuffer<f32>>; 2] = [None, None];
///
///         // Before we can process the buffers, we need to check all the necessary channels
///         // are present, in case the host messed up the port configuration.
///         // (Yes, some do.)
///         for ((channel_pair, in_buf), out_buf) in channel_pairs
///             .iter()
///             .zip(&mut input_channels)
///             .zip(&mut output_channels)
///         {
///             // A separate pair of channels is just what we want for this example!
///             // However, a real plugin implementation would need to gracefully handle
///             // the other cases, including having spare input buffers ready for
///             // in-place processing.
///             if let ChannelPair::InputOutput(input, output) = channel_pair {
///                 *in_buf = Some(input);
///                 *out_buf = Some(output);
///             }
///         }
///
///         // Channel swap! (assuming all channels are there)
///         if let (Some(in_l), Some(out_r)) = (input_channels[0], output_channels[1]) {
///             out_r.copy_from_buffer(in_l)
///         }
///         if let (Some(in_r), Some(out_l)) = (input_channels[1], output_channels[0]) {
///             out_l.copy_from_buffer(in_r)
///         }
///     }
///
///     Ok(ProcessStatus::Continue)
/// }
/// ```
#[derive(Copy, Clone)]
pub struct Audio<'a> {
    inputs: &'a [CelledClapAudioBuffer],
    outputs: &'a [CelledClapAudioBuffer],
    frames_count: u32,
}

impl<'a> Audio<'a> {
    /// Create a new [`Audio`] from the information in a given raw, C-FFI compatible `clap_process`
    /// struct.
    ///
    /// # Safety
    ///
    /// Users must ensure all fields of the given `raw_process` are valid, and that all buffers
    /// it points to remain valid for the given `'a` lifetime.
    #[inline]
    pub unsafe fn from_raw(raw_process: &clap_process) -> Audio<'a> {
        Audio {
            frames_count: raw_process.frames_count,
            inputs: slice_from_external_parts(
                raw_process.audio_inputs.cast(),
                raw_process.audio_inputs_count as usize,
            ),
            outputs: slice_from_external_parts(
                raw_process.audio_outputs.cast(),
                raw_process.audio_outputs_count as usize,
            ),
        }
    }

    /// Create a new [`Audio`] from raw buffer structs.
    ///
    /// # Safety
    ///
    /// The caller must ensure the given pointers to all buffer structs are valid for both reads
    /// and writes for the duration of 'a, including all the buffer pointers they themselves contain.
    ///
    /// The caller must also ensure `frames_count` is lower than or equal to the sizes of the
    /// channel buffers pointed to by `buffers`.
    #[inline]
    pub unsafe fn from_raw_buffers(
        inputs: *mut [clap_audio_buffer],
        outputs: *mut [clap_audio_buffer],
        frames_count: u32,
    ) -> Self {
        Self {
            inputs: &*(inputs as *const _),
            outputs: &*(outputs as *const _),
            frames_count,
        }
    }

    /// Returns a raw pointer to a slice of the raw input buffers structs.
    ///
    /// # Safety
    ///
    /// While this function is safe to use, there are many cases where using the resulting pointer
    /// is not.
    ///
    /// This is because the contents slice of buffer structs (as well as the raw audio buffers these
    /// point to) are valid for both reads and writes from other, potentially aliased pointers to
    /// that data.
    ///
    /// This means it is not valid to create either shared (`&`) or mutable (`&mut`) Rust references
    /// to these buffers or their data.
    ///
    /// In order to safely access the data, you can either use [`Cell`]s, or perform direct
    /// read or write operations, e.g. using [`ptr::read`] or [`ptr::write`].
    ///
    /// [`ptr::read`]: core::ptr::read
    /// [`ptr::write`]: core::ptr::write
    /// [`Cell`]: core::cell::Cell
    #[inline]
    pub fn raw_inputs(&self) -> *mut [clap_audio_buffer] {
        core::ptr::slice_from_raw_parts_mut(
            self.inputs.as_ptr().cast_mut().cast(),
            self.inputs.len(),
        )
    }

    /// Returns a raw pointer to a C array of the raw output buffers structs.
    ///
    /// # Safety
    ///
    /// While this function is safe to use, there are many cases where using the resulting pointer
    /// is not.
    ///
    /// This is because the contents slice of buffer structs (as well as the raw audio buffers these
    /// point to) are valid for both reads and writes from other, potentially aliased pointers to
    /// that data.
    ///
    /// This means it is not valid to create either shared (`&`) or mutable (`&mut`) Rust references
    /// to these buffers or their data.
    ///
    /// In order to safely access the data, you can either use [`Cell`]s, or perform direct
    /// read or write operations, e.g. using [`ptr::read`] or [`ptr::write`].
    ///
    /// [`ptr::read`]: core::ptr::read
    /// [`ptr::write`]: core::ptr::write
    /// [`Cell`]: core::cell::Cell
    #[inline]
    pub fn raw_outputs(&self) -> *mut [clap_audio_buffer] {
        core::ptr::slice_from_raw_parts_mut(
            self.outputs.as_ptr().cast_mut().cast(),
            self.outputs.len(),
        )
    }

    /// Retrieves the input [`Port`] at a given index.
    ///
    /// This returns [`None`] if there is no input port at the given index.
    ///
    /// See also the [`input_port_count`](Audio::input_port_count) method to know how many input
    /// ports are available, and the [`input_ports`](Audio::input_ports) method to get all input ports at once.
    #[inline]
    pub fn input_port(&self, index: usize) -> Option<Port<'a>> {
        self.inputs
            .get(index)
            // SAFETY: this type ensures the provided buffer is valid and frames_count is correct
            .map(|buf| unsafe { Port::from_raw(buf, self.frames_count) })
    }

    /// Retrieves the number of available input [`Port`]s.
    #[inline]
    pub fn input_port_count(&self) -> usize {
        self.inputs.len()
    }

    /// Returns an iterator of all the available input [`Port`]s at once.
    ///
    /// See also the [`input_port`](Audio::input_port) method to retrieve a single input port by
    /// its index.
    #[inline]
    pub fn input_ports(&self) -> PortsIter<'a> {
        PortsIter::new(self.inputs, self.frames_count)
    }

    /// Retrieves the output [`Port`] at a given index.
    ///
    /// This returns [`None`] if there is no output port at the given index.
    ///
    /// See also the [`output_port_count`](Audio::output_port_count) method to know how many output
    /// ports are available, and the [`output_ports`](Audio::output_ports) method to get all output ports at once.
    #[inline]
    pub fn output_port(&self, index: usize) -> Option<Port<'a>> {
        self.outputs
            .get(index)
            // SAFETY: this type ensures the provided buffer is valid and frames_count is correct.
            .map(|buf| unsafe { Port::from_raw(buf, self.frames_count) })
    }

    /// Retrieves the number of available output [`Port`]s.
    #[inline]
    pub fn output_port_count(&self) -> usize {
        self.outputs.len()
    }

    /// Returns an iterator of all the available output [`Port`]s at once.
    ///
    /// See also the [`output_port`](Audio::output_port) method to retrieve a single output port by
    /// its index.
    #[inline]
    pub fn output_ports(&self) -> PortsIter<'a> {
        PortsIter::new(self.outputs, self.frames_count)
    }

    /// Retrieves the [`PortPair`] at a given index.
    ///
    /// This returns [`None`] if there is no available port at the given index.
    ///
    /// See also the [`port_pair_count`](Audio::port_pair_count) method to know how many port
    /// pairs are available, and the [`port_pairs`](Audio::port_pairs) method to get all port pairs at once.
    #[inline]
    pub fn port_pair(&self, index: usize) -> Option<PortPair<'a>> {
        // SAFETY: this type ensures the provided buffers are valid and frames_count is correct
        unsafe {
            PortPair::from_raw(
                self.inputs.get(index),
                self.outputs.get(index),
                self.frames_count,
            )
        }
    }

    /// Retrieves the number of available [`PortPair`]s.
    ///
    /// Because [`PortPair`] can be mismatched (i.e. have an input but no output, or vice-versa),
    /// this is effectively equal to the maximum number of ports available, either on the input side
    /// or the output side.
    #[inline]
    pub fn port_pair_count(&self) -> usize {
        self.input_port_count().max(self.output_port_count())
    }

    /// Returns an iterator of all the available [`PortPair`]s at once.
    ///
    /// See also the [`port_pair`](Audio::port_pair) method to retrieve a single input port by
    /// its index.
    #[inline]
    pub fn port_pairs(&self) -> PortPairsIter<'a> {
        PortPairsIter::new(self)
    }

    /// Returns a sub-range of ports as a new [`Audio`] struct, similar to a subslice of items.
    #[inline]
    pub fn port_sub_range(&self, range: impl RangeBounds<usize> + Clone) -> Audio<'a> {
        let inputs = self
            .inputs
            .get((range.start_bound().cloned(), range.end_bound().cloned()))
            .unwrap_or(&[]);

        let outputs = self
            .outputs
            .get((range.start_bound().cloned(), range.end_bound().cloned()))
            .unwrap_or(&[]);

        Audio {
            inputs,
            outputs,
            frames_count: self.frames_count,
        }
    }

    /// Returns the number of frames to process in this block.
    ///
    /// This will always match the number of samples of every audio buffer in this [`Audio`] struct.
    #[inline]
    pub fn frames_count(&self) -> u32 {
        self.frames_count
    }
}

impl<'a> IntoIterator for Audio<'a> {
    type Item = PortPair<'a>;
    type IntoIter = PortPairsIter<'a>;

    /// Returns an iterator of all the available [`PortPair`]s at once. This is equivalent to using
    /// [`port_pairs`](Audio::port_pairs).
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.port_pairs()
    }
}

impl<'a> IntoIterator for &Audio<'a> {
    type Item = PortPair<'a>;
    type IntoIter = PortPairsIter<'a>;

    /// Returns an iterator of all the available [`PortPair`]s at once. This is equivalent to using
    /// [`port_pairs`](Audio::port_pairs).
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.port_pairs()
    }
}
