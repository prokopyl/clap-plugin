use crate::internal_utils::slice_from_external_parts;
use crate::process::audio::{BufferError, CelledClapAudioBuffer};

/// A generic enum to discriminate between buffers containing [`f32`] and [`f64`] sample types.
///
/// This enum is actually just a generic container for two different types, called `F32` and `F64`,
/// representing a buffer that's holding either [`f32`] or [`f64`] sample data.
///
/// This type is used by methods that detect which types of sample buffers are available:
///
/// * [`Port::channels`](super::Port::channels) returns a [`SampleType`] of
///   [`Channels`](super::Channels);
/// * [`PortPair::channels`](super::PortPair::channels) returns a [`SampleType`] of
///   [`ChannelsPairs`](super::ChannelsPairs);
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum SampleType<F32, F64> {
    /// Only [`f32`] sample buffers are available.
    F32(F32),
    /// Only [`f64`] sample buffers are available.
    F64(F64),
    /// Both [`f32`] and [`f64`] sample buffers are available.
    ///
    /// The host isn't actually allowed to send both buffer types for a given port to the plugin,
    /// but this is still technically a possibility due to how the CLAP API is designed.
    ///
    /// This variant is there to let plugins decide what to do with the buffers and whether
    /// to simply ignore the extra one, instead of throwing a hard error if this were to happen.
    Both(F32, F64),
}

impl<F32, F64> SampleType<F32, F64> {
    /// Returns a reference to the `F32` sample buffer type if it is available,
    /// or [`None`] otherwise.
    ///
    /// If both buffer types are available, the `F64` buffer type is ignored.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::process::audio::SampleType;
    ///
    /// assert_eq!(SampleType::<f32, f64>::F32(1.0).to_f32(), Some(1.0f32));
    /// assert_eq!(SampleType::<f32, f64>::F64(1.0).to_f32(), None);
    /// assert_eq!(SampleType::<f32, f64>::Both(1.0, 1.0).to_f32(), Some(1.0f32));
    /// ```
    #[inline]
    pub fn to_f32(&self) -> Option<F32>
    where
        F32: Copy,
    {
        match self {
            SampleType::F32(c) | SampleType::Both(c, _) => Some(*c),
            _ => None,
        }
    }

    /// Returns a reference to the `F64` sample buffer type if it is available,
    /// or [`None`] otherwise.
    ///
    /// If both buffer types are available, the `F32` buffer type is ignored.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::process::audio::SampleType;
    ///
    /// assert_eq!(SampleType::<f32, f64>::F32(1.0).to_f64(), None);
    /// assert_eq!(SampleType::<f32, f64>::F64(1.0).to_f64(), Some(1.0f64));
    /// assert_eq!(SampleType::<f32, f64>::Both(1.0, 1.0).to_f64(), Some(1.0f64));
    /// ```
    #[inline]
    pub fn to_f64(&self) -> Option<F64>
    where
        F64: Copy,
    {
        match self {
            SampleType::F64(c) | SampleType::Both(_, c) => Some(*c),
            _ => None,
        }
    }

    /// Maps a `SampleType<F32, F64>` to `SampleType<TF32, TF64>` by applying functions to the
    /// contained values.
    ///
    /// The `fn32` function parameter is applied to the `F32` buffer type if it is present, and the
    /// `fn64` function parameter is applied to the `F64` buffer type if it is present. If both
    /// buffer types are present, both functions are applied to each matching value type.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::process::audio::SampleType;
    ///
    /// assert_eq!(
    ///     SampleType::<f32, f64>::F32(1.0).map(|f| f * 2.0, |f| f * 3.0),
    ///     SampleType::F32(2f32)
    /// );
    /// assert_eq!(
    ///     SampleType::<f32, f64>::F64(1.0).map(|f| f * 2.0, |f| f * 3.0),
    ///     SampleType::F64(3f64)
    /// );
    /// assert_eq!(
    ///     SampleType::<f32, f64>::Both(1.0, 1.0).map(|f| f * 2.0, |f| f * 3.0),
    ///     SampleType::Both(2f32, 3f64)
    /// );
    /// ```
    #[inline]
    pub fn map<TF32, TF64, Fn32, Fn64>(self, fn32: Fn32, fn64: Fn64) -> SampleType<TF32, TF64>
    where
        Fn32: FnOnce(F32) -> TF32,
        Fn64: FnOnce(F64) -> TF64,
    {
        match self {
            SampleType::F32(c) => SampleType::F32(fn32(c)),
            SampleType::F64(c) => SampleType::F64(fn64(c)),
            SampleType::Both(c32, c64) => SampleType::Both(fn32(c32), fn64(c64)),
        }
    }

    /// Tries to match two `SampleType`s possibly containing different buffer types.
    ///
    /// This returns a `SampleType` containing a tuple of the matched buffer types (i.e.
    /// `(F32, TF32)` if both 32-bit buffer types were present, or `(F64, TF64)` if both 32-bit
    /// buffer types were present).
    ///
    /// If any of the two `SampleType`s actually contain [`Both`](SampleType::Both), then the extra
    /// buffer type is discarded. Unless both `SampleType`s contain both buffers, in which case
    /// [`Both`](SampleType::Both) is also returned.
    ///
    /// This method consumes the `SampleType`.
    ///
    /// # Errors
    ///
    /// This method returns a [`BufferError::MismatchedBufferPair`] if no match could be found, i.e.
    /// one`SampleType` contains only a `F32` and the other contains only a `F64`.
    ///
    /// # Examples
    ///
    /// ```
    /// use clack_plugin::process::audio::{BufferError, SampleType};
    ///
    /// assert_eq!(
    ///     SampleType::<f32, f64>::F32(1.0)
    ///         .try_match_with(SampleType::<f32, f64>::F32(2.0)),
    ///     Ok(SampleType::F32((1.0, 2.0)))
    /// );
    /// assert_eq!(
    ///     SampleType::<f32, f64>::F64(1.0)
    ///         .try_match_with(SampleType::<f32, f64>::F64(2.0)),
    ///     Ok(SampleType::F64((1.0, 2.0)))
    /// );
    ///
    /// assert_eq!(
    ///     SampleType::<f32, f64>::F32(1.0)
    ///         .try_match_with(SampleType::<f32, f64>::F64(2.0)),
    ///     Err(BufferError::MismatchedBufferPair)
    /// );
    /// ```
    #[inline]
    #[allow(clippy::type_complexity)] // I'm sorry
    pub fn try_match_with<TF32, TF64>(
        self,
        other: SampleType<TF32, TF64>,
    ) -> Result<SampleType<(F32, TF32), (F64, TF64)>, BufferError> {
        match (self, other) {
            (SampleType::Both(s32, s64), SampleType::Both(o32, o64)) => {
                Ok(SampleType::Both((s32, o32), (s64, o64)))
            }
            (
                SampleType::F32(s) | SampleType::Both(s, _),
                SampleType::F32(o) | SampleType::Both(o, _),
            ) => Ok(SampleType::F32((s, o))),
            (
                SampleType::F64(s) | SampleType::Both(_, s),
                SampleType::F64(o) | SampleType::Both(_, o),
            ) => Ok(SampleType::F64((s, o))),
            _ => Err(BufferError::MismatchedBufferPair),
        }
    }
}

impl<'a> SampleType<&'a [*mut f32], &'a [*mut f64]> {
    /// # Safety
    ///
    /// The caller must ensure the provided buffer is valid.
    #[inline]
    pub(crate) unsafe fn from_raw_buffer(raw: &CelledClapAudioBuffer) -> Result<Self, BufferError> {
        match (raw.data32.is_null(), raw.data64.is_null()) {
            (true, true) => {
                if raw.channel_count == 0 {
                    Ok(SampleType::Both([].as_slice(), [].as_slice()))
                } else {
                    Err(BufferError::InvalidChannelBuffer)
                }
            }
            (false, true) => Ok(SampleType::F32(slice_from_external_parts(
                raw.data32 as *const *mut f32,
                raw.channel_count as usize,
            ))),
            (true, false) => Ok(SampleType::F64(slice_from_external_parts(
                raw.data64 as *const *mut f64,
                raw.channel_count as usize,
            ))),
            (false, false) => Ok(SampleType::Both(
                slice_from_external_parts(
                    raw.data32 as *const *mut f32,
                    raw.channel_count as usize,
                ),
                slice_from_external_parts(
                    raw.data64 as *const *mut f64,
                    raw.channel_count as usize,
                ),
            )),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn try_match_with() {
        assert_eq!(
            SampleType::<f32, f64>::F32(1.0).try_match_with(SampleType::<f32, f64>::F32(2.0)),
            Ok(SampleType::F32((1.0, 2.0)))
        );
        assert_eq!(
            SampleType::<f32, f64>::F64(1.0).try_match_with(SampleType::<f32, f64>::F64(2.0)),
            Ok(SampleType::F64((1.0, 2.0)))
        );

        assert_eq!(
            SampleType::<f32, f64>::F32(1.0).try_match_with(SampleType::<f32, f64>::F64(2.0)),
            Err(BufferError::MismatchedBufferPair)
        );

        assert_eq!(
            SampleType::<f32, f64>::Both(1.0, 2.0).try_match_with(SampleType::<f32, f64>::F32(3.0)),
            Ok(SampleType::F32((1.0, 3.0)))
        );
        assert_eq!(
            SampleType::<f32, f64>::Both(1.0, 2.0).try_match_with(SampleType::<f32, f64>::F64(4.0)),
            Ok(SampleType::F64((2.0, 4.0)))
        );
        assert_eq!(
            SampleType::<f32, f64>::Both(1.0, 2.0)
                .try_match_with(SampleType::<f32, f64>::Both(3.0, 4.0)),
            Ok(SampleType::Both((1.0, 3.0), (2.0, 4.0)))
        );
    }
}
