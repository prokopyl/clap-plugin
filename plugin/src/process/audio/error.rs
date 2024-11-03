use std::error::Error;
use std::fmt::{Display, Formatter};

/// Errors that can occur when accessing [`Audio`](crate::process::Audio) buffers.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BufferError {
    /// A port's channel buffers are invalid.
    ///
    /// This happens when both the [`f32`] and [`f64`] buffer pointers provided by the host are null.
    ///
    /// This error can be returned by the [`InputPort::channels`](super::Port::channels) or
    /// [`PortPair::channels`](super::PortPair::channels) methods.
    InvalidChannelBuffer,
    /// A pair of mismatched buffer types (i.e. one [`f32`] and the other [`f64`]) were tried to
    /// be accessed together.
    ///
    /// This error is returned by the [`PortPair::channels`](super::PortPair::channels) method if
    /// the two ports in the pair have mismatched types.
    ///
    /// This error is also used by the [`SampleType::try_match_with`](super::SampleType::try_match_with) method.
    MismatchedBufferPair,
}

impl Display for BufferError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BufferError::InvalidChannelBuffer => {
                f.write_str("Invalid port channels buffers: both the data32 and data64 pointers were null")
            },
            BufferError::MismatchedBufferPair => f.write_str("Invalid channel buffer pairing: attempted to read/write a 32-bit buffer and a 64-bit buffer together")
        }
    }
}

impl Error for BufferError {}
