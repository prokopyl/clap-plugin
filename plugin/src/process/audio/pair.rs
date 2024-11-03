use crate::process::audio::pair::ChannelPair::*;
use crate::process::audio::{
    AudioBuffer, BufferError, CelledClapAudioBuffer, Channels, Port, SampleType,
};
use crate::process::Audio;
use std::slice::Iter;

/// A pair of Input and Output ports.
///
/// Note that in case of asymmetric port layouts (e.g. side-chain), there may not
/// be an associated output port to an input port, or vice-versa. The same goes for
/// paired channels.
///
/// In those cases, a given pair will only contain one port instead of two. However,
/// a [`PortPair`] is always guaranteed to contain at least one port, be it an input or output.
#[derive(Copy, Clone)]
pub struct PortPair<'a> {
    input: Option<&'a CelledClapAudioBuffer>,
    output: Option<&'a CelledClapAudioBuffer>,
    frames_count: u32,
}

impl<'a> PortPair<'a> {
    /// # Safety
    ///
    /// * Both provided buffers must be valid (if not `None`);
    /// * `frames_count` *must* match the size of the buffers.
    #[inline]
    pub(crate) unsafe fn from_raw(
        input: Option<&'a CelledClapAudioBuffer>,
        output: Option<&'a CelledClapAudioBuffer>,
        frames_count: u32,
    ) -> Option<Self> {
        match (input, output) {
            (None, None) => None,
            (input, output) => Some(PortPair {
                input,
                output,
                frames_count,
            }),
        }
    }

    /// Gets the input [`Port`] of this pair.
    ///
    /// If the port layout is asymmetric and there is no input port, this returns [`None`].
    #[inline]
    pub fn input(&self) -> Option<Port<'a>> {
        self.input
            // SAFETY: this type ensures the buffer is valid and matches frame_count
            .map(|i| unsafe { Port::from_raw(i, self.frames_count) })
    }

    /// Gets the output [`Port`] of this pair.
    ///
    /// If the port layout is asymmetric and there is no output port, this returns [`None`].
    #[inline]
    pub fn output(&self) -> Option<Port<'a>> {
        self.output
            // SAFETY: this type ensures the buffer is valid and matches frame_count
            .map(|i| unsafe { Port::from_raw(i, self.frames_count) })
    }

    /// Retrieves the port pair's channels.
    ///
    /// Because each port can hold either [`f32`] or [`f64`] sample data, this method returns a
    /// [`SampleType`] enum of the paired channels, to indicate which one the ports holds.
    ///
    /// # Errors
    ///
    /// This method returns a [`BufferError::InvalidChannelBuffer`] if the host provided neither
    /// [`f32`] nor [`f64`] buffer type, which is invalid per the CLAP specification.
    ///
    /// Additionally, if the two port have different buffer sample types (i.e. one holds [`f32`]
    /// and the other holds [`f64`]), then a [`BufferError::MismatchedBufferPair`] error is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::process::audio::{ChannelsPairs, PortPair, SampleType};
    ///
    /// # fn foo(port: PortPair) {
    /// let mut port: PortPair = /* ... */
    /// # port;
    ///
    /// // Decide what to do using by matching against every possible configuration
    /// match port.channels().unwrap() {
    ///     SampleType::F32(buf) => { /* Process the 32-bit buffers */ },
    ///     SampleType::F64(buf) => { /* Process the 64-bit buffers */ },
    ///     SampleType::Both(buf32, buf64) => { /* We have both types of buffers available */ }
    /// }
    ///
    /// // If we're only interested in a single buffer type,
    /// // we can use SampleType's helper methods:
    /// let channels: ChannelsPairs<f32> = port.channels().unwrap().to_f32().unwrap();
    /// # }
    /// ```
    #[inline]
    pub fn channels(
        &self,
    ) -> Result<SampleType<ChannelsPairs<'a, f32>, ChannelsPairs<'a, f64>>, BufferError> {
        let input = match self.input {
            None => SampleType::Both([].as_slice(), [].as_slice()),
            // SAFETY: this type ensures the buffer is valid
            Some(buffer) => unsafe { SampleType::from_raw_buffer(buffer)? },
        };

        let output = match self.output {
            None => SampleType::Both([].as_slice(), [].as_slice()),
            // SAFETY: this type ensures the buffer is valid
            Some(buffer) => unsafe { SampleType::from_raw_buffer(buffer)? },
        };

        Ok(input.try_match_with(output)?.map(
            |(i, o)| ChannelsPairs {
                inputs: i,
                outputs: o,
                frames_count: self.frames_count,
            },
            |(i, o)| ChannelsPairs {
                inputs: i,
                outputs: o,
                frames_count: self.frames_count,
            },
        ))
    }

    /// The number of channels in this port pair.
    ///
    /// Since there may be more channels in one port than in the other, this method also counts
    /// the partial [`ChannelPair`]s that can be returned, and therefore returns the maximum number
    /// of channels between the two ports.  
    #[inline]
    pub fn channel_pair_count(&self) -> usize {
        let in_channels = self.input.map(|b| b.channel_count).unwrap_or(0);
        let out_channels = self.output.as_ref().map(|b| b.channel_count).unwrap_or(0);

        in_channels.max(out_channels) as usize
    }

    /// Returns the number of frames to process in this block.
    ///
    /// This will always match the number of samples of every audio channel buffer. The two ports
    /// *cannot* have different buffer sizes.
    #[inline]
    pub fn frames_count(&self) -> u32 {
        self.frames_count
    }
}

/// An [`PortPair`]'s channels' data buffers, which contains samples of a given type `S`.
///
/// The sample type `S` is always going to be either [`f32`] or [`f64`], as returned by
/// [`PortPair::channels`].
pub struct ChannelsPairs<'a, S> {
    inputs: &'a [*mut S],
    outputs: &'a [*mut S],
    frames_count: u32,
}

impl<'a, S> ChannelsPairs<'a, S> {
    /// Creates a new pair of [`Channels`] list from an input and output channels list.
    ///
    /// # Panics
    ///
    /// This function will panic if `inputs` and `outputs` don't have the same `frame_count`.
    pub fn from_channels(inputs: Channels<'a, S>, outputs: Channels<'a, S>) -> Self {
        if inputs.frames_count() != outputs.frames_count() {
            mismatched_frames_count(inputs.frames_count(), outputs.frames_count())
        }

        #[inline(never)]
        #[cold]
        fn mismatched_frames_count(input_frames: u32, output_frames: u32) -> ! {
            panic!(
                "Tried to pair two channels with different frame counts (input: {}, output: {}.",
                input_frames, output_frames
            )
        }

        Self {
            inputs: inputs.raw_data(),
            outputs: outputs.raw_data(),
            frames_count: inputs.frames_count(),
        }
    }

    /// Returns the number of frames to process in this block.
    ///
    /// This will always match the number of samples of every audio channel buffer, from both
    /// the input and output port.
    #[inline]
    pub fn frames_count(&self) -> u32 {
        self.frames_count
    }

    /// The number of input channels.
    #[inline]
    pub fn input_channel_count(&self) -> usize {
        self.inputs.len()
    }

    /// The number of output channels.
    #[inline]
    pub fn output_channel_count(&self) -> usize {
        self.outputs.len()
    }

    /// The total number of channel pairs.
    ///
    /// Since there may be more channels in one port than in the other, this method also counts
    /// the partial [`ChannelPair`]s that can be returned, and therefore returns the maximum number
    /// of channels between the input and output ports.  
    #[inline]
    pub fn channel_pair_count(&self) -> usize {
        self.input_channel_count().max(self.output_channel_count())
    }

    /// Returns the input channels.
    pub fn input_channels(&self) -> Channels<'a, S> {
        // SAFETY: The input_data and frames_count fields are guaranteed to be valid
        unsafe { Channels::from_raw(self.inputs, self.frames_count) }
    }

    /// Returns the output channels.
    pub fn output_channels(&self) -> Channels<'a, S> {
        // SAFETY: The input_data and frames_count fields are guaranteed to be valid
        unsafe { Channels::from_raw(self.outputs, self.frames_count) }
    }

    /// Retrieves the pair of sample buffers for the pair of channels at a given index.
    ///
    /// If there is no channel at the given index (i.e. `channel_index` is greater or equal than
    /// [`channel_pair_count`](Self::channel_pair_count)), this returns [`None`].
    ///
    /// See [`ChannelPair`]'s documentation for examples on how to access sample buffers from it.
    #[inline]
    pub fn channel_pair(&self, index: usize) -> Option<ChannelPair<'a, S>> {
        let input = self
            .inputs
            .get(index)
            // SAFETY: this type ensures the pointer is valid and the slice is frames_count-long
            .map(|ptr| unsafe { AudioBuffer::from_raw_parts(*ptr, self.frames_count as usize) });

        let output = self
            .outputs
            .get(index)
            // SAFETY: this type ensures the pointer is valid and the slice is frames_count-long
            .map(|ptr| unsafe { AudioBuffer::from_raw_parts(*ptr, self.frames_count as usize) });

        ChannelPair::from_optional_io(input, output)
    }

    /// Gets an iterator over all the ports' [`ChannelPair`]s.
    #[inline]
    pub fn iter(&self) -> ChannelsPairsIter<'a, S> {
        ChannelsPairsIter {
            input_iter: self.inputs.iter(),
            output_iter: self.outputs.iter(),
            frames_count: self.frames_count,
        }
    }
}

impl<'a, S> Copy for ChannelsPairs<'a, S> {}
impl<'a, S> Clone for ChannelsPairs<'a, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, S> IntoIterator for ChannelsPairs<'a, S> {
    type Item = ChannelPair<'a, S>;
    type IntoIter = ChannelsPairsIter<'a, S>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, S> IntoIterator for &ChannelsPairs<'a, S> {
    type Item = ChannelPair<'a, S>;
    type IntoIter = ChannelsPairsIter<'a, S>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over all of a [`PortPair`]'s [`ChannelPair`]s.
pub struct ChannelsPairsIter<'a, S> {
    input_iter: Iter<'a, *mut S>,
    output_iter: Iter<'a, *mut S>,
    frames_count: u32,
}

impl<'a, S> Iterator for ChannelsPairsIter<'a, S> {
    type Item = ChannelPair<'a, S>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let input = self
            .input_iter
            .next()
            // SAFETY: this type ensures the pointer is valid and the slice is frames_count-long
            .map(|ptr| unsafe { AudioBuffer::from_raw_parts(*ptr, self.frames_count as usize) });

        // SAFETY: this type ensures the pointer is valid and the slice is frames_count-long
        let output = self.output_iter.next().map(|ptr| unsafe {
            AudioBuffer::from_raw_parts((*ptr) as *mut _, self.frames_count as usize)
        });

        ChannelPair::from_optional_io(input, output)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

impl<S> ExactSizeIterator for ChannelsPairsIter<'_, S> {
    #[inline]
    fn len(&self) -> usize {
        self.input_iter.len().max(self.output_iter.len())
    }
}

impl<'a, S> Clone for ChannelsPairsIter<'a, S> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            input_iter: self.input_iter.clone(),
            output_iter: self.output_iter.clone(),
            frames_count: self.frames_count,
        }
    }
}

/// An iterator of all of the available [`PortPair`]s from an [`Audio`] struct.
#[derive(Clone)]
pub struct PortPairsIter<'a> {
    inputs: Iter<'a, CelledClapAudioBuffer>,
    outputs: Iter<'a, CelledClapAudioBuffer>,
    frames_count: u32,
}

impl<'a> PortPairsIter<'a> {
    #[inline]
    pub(crate) fn new(audio: &Audio<'a>) -> Self {
        Self {
            inputs: audio.inputs.iter(),
            outputs: audio.outputs.iter(),
            frames_count: audio.frames_count,
        }
    }
}

impl<'a> Iterator for PortPairsIter<'a> {
    type Item = PortPair<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: the audio type this is created from
        unsafe { PortPair::from_raw(self.inputs.next(), self.outputs.next(), self.frames_count) }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

impl ExactSizeIterator for PortPairsIter<'_> {
    #[inline]
    fn len(&self) -> usize {
        self.inputs.len().max(self.outputs.len())
    }
}

/// A pair of input and output channel buffers.
///
/// Because port and channel layouts may differ between inputs and outputs, a [`ChannelPair`]
/// may actually contain only an input or an output instead of both.
///
/// This enum also allows to check for the fairly common case where Host may re-use the same
/// buffer for both the input and output, and expect the plugin to do in-place processing.
#[derive(Copy, Clone)]
pub enum ChannelPair<'a, S> {
    /// There is only an input channel present, there was no matching output.
    InputOnly(&'a AudioBuffer<S>),
    /// There is only an output channel present, there was no matching input.
    OutputOnly(&'a AudioBuffer<S>),
    /// Both the input and output channels are present, and available separately.
    InputOutput(&'a AudioBuffer<S>, &'a AudioBuffer<S>),
    /// Both the input and output channels are present, but they actually share the same buffer.
    ///
    /// In this case, the slice is already filled with the input channel's data, and the host
    /// considers the contents of this buffer after processing to be the output channel's data.
    InPlace(&'a AudioBuffer<S>),
}

impl<'a, S> ChannelPair<'a, S> {
    #[inline]
    pub(crate) fn from_optional_io(
        input: Option<&'a AudioBuffer<S>>,
        output: Option<&'a AudioBuffer<S>>,
    ) -> Option<ChannelPair<'a, S>> {
        match (input, output) {
            (None, None) => None,
            (Some(input), None) => Some(InputOnly(input)),
            (None, Some(output)) => Some(OutputOnly(output)),
            (Some(input), Some(output)) => Some(if input.as_ptr() == output.as_ptr() {
                InPlace(output)
            } else {
                InputOutput(input, output)
            }),
        }
    }

    /// Attempts to retrieve the input channel's buffer data, if the input channel is present.
    #[inline]
    pub fn input(&self) -> Option<&'a AudioBuffer<S>> {
        match self {
            InputOnly(i) | InputOutput(i, _) => Some(*i),
            OutputOnly(_) => None,
            InPlace(io) => Some(*io),
        }
    }

    /// Attempts to retrieve a read-only reference to the output channel's buffer data,
    /// if the output channel is present.
    #[inline]
    pub fn output(&'a self) -> Option<&'a AudioBuffer<S>> {
        match self {
            OutputOnly(o) | InputOutput(_, o) | InPlace(o) => Some(*o),
            InputOnly(_) => None,
        }
    }
}
