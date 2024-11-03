use core::cell::Cell;
use core::fmt::{Debug, Formatter};
use core::ops::Index;
use core::ptr;
use core::slice::SliceIndex;

/// A read/write buffer of audio samples of type `S`.
///
/// Unlike the `&[T]` or `&mut [T]` slice types that can be used to represent contiguous audio
/// buffers, a single reference to an [`AudioBuffer`] can be both shared *and* mutable.
///
/// However, because they allow shared mutability, it is not possible to obtain a reference
/// (shared or mutable) to a value in an [`AudioBuffer`], nor is it possible to get standard slices
/// from an [`AudioBuffer`] reference, as the values may be modified from any other reference to the
/// buffer.
///
/// One can create a shareable [`AudioBuffer`] reference from a mutable slice reference (`&mut [T]`)
/// using the [`from_mut_slice`](AudioBuffer::from_mut_slice) method.
/// Alternatively, it is also possible to convert between [`AudioBuffer`] and slice of
/// [`Cell`s](Cell) references with the [`from_slice_of_cells`](AudioBuffer::from_slice_of_cells) and
/// [`as_slice_of_cells`](AudioBuffer::as_slice_of_cells) methods.
///
/// [`AudioBuffer`s](AudioBuffer) can also be indexed into and sub-sliced like normal slices using
/// the [`Index`] operator.
///
/// As long as the sample type `S` is `Copy`, the following operations are also directly available:
///
/// * Reading sample data, using [`get`], [`try_get`] or [`get_unchecked`];
/// * Writing sample data, using [`put`] or [`put_unchecked`];
/// * Copying between buffers, using [`copy_from_buffer`] or [`copy_to_buffer`];
/// * Copying from and to regular slices, using [`copy_from_slice`] or [`copy_to_slice`];
/// * Filling the buffer with a single value using [`fill`].
///
/// # Example
///
/// ```
/// use clack_plugin::prelude::AudioBuffer;
/// let mut data = [0.0, 1.0, 2.0, 3.0, 4.0];
///
/// let buf1: &AudioBuffer<f32> = AudioBuffer::from_mut_slice(&mut data);
/// let buf2 = buf1;
///
/// buf2.put(1, 11.0);
/// buf1.put(2, 22.0);
///
/// assert_eq!(buf1, &[0.0, 11.0, 22.0, 3.0, 4.0]);
/// ```
///
/// ```
/// use clack_plugin::prelude::AudioBuffer;
///
/// // By taking AudioBuffers, this functions supports in-place processing.
/// fn double_input(input: &AudioBuffer<f32>, output: &AudioBuffer<f32>) {
///     for (i, o) in input.iter().zip(output) {
///         o.set(i.get() * 2.0)
///     }
/// }
///
/// let mut input = [0.0, 1.0, 2.0, 3.0];
/// let mut output = [0.0; 4];
///
/// let input_buf: &AudioBuffer<f32> = AudioBuffer::from_mut_slice(&mut input);
/// let output_buf: &AudioBuffer<f32> = AudioBuffer::from_mut_slice(&mut output);
///
/// double_input(input_buf, output_buf);
/// assert_eq!(output, [0.0, 2.0, 4.0, 6.0]);
///
/// // Processes the data in-place with a single buffer for both input and output.
/// double_input(input_buf, input_buf);
/// assert_eq!(input, [0.0, 2.0, 4.0, 6.0]); // Input buffer has been modified.
///
/// ```
///
/// [`get`]: AudioBuffer::get
/// [`try_get`]: AudioBuffer::try_get
/// [`get_unchecked`]: AudioBuffer::get_unchecked
///
/// [`put`]: AudioBuffer::put
/// [`put_unchecked`]: AudioBuffer::put_unchecked
///
/// [`copy_from_buffer`]: AudioBuffer::copy_from_buffer
/// [`copy_to_buffer`]: AudioBuffer::copy_to_buffer
/// [`copy_from_slice`]: AudioBuffer::copy_from_slice
/// [`copy_to_slice`]: AudioBuffer::copy_to_slice
/// [`fill`]: AudioBuffer::fill
#[repr(transparent)]
pub struct AudioBuffer<S> {
    inner: [Cell<S>],
}

impl<S> AudioBuffer<S> {
    /// Creates a reference to an empty buffer.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// let buf: &AudioBuffer<f32> = AudioBuffer::empty();
    ///
    /// assert_eq!(buf.len(), 0)
    /// ```
    #[inline]
    #[must_use]
    pub const fn empty() -> &'static Self {
        Self::from_slice_of_cells(&[])
    }

    /// Creates a buffer reference from a mutable reference to a slice of samples.
    ///
    /// If you do not have exclusive (`&mut`) access to the sample data, you may consider using
    /// [`AudioBuffer::from_slice_of_cells`] instead.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// let mut data = [0.0, 1.0, 2.0];
    ///
    /// let buf: &AudioBuffer<f32> = AudioBuffer::from_mut_slice(&mut data);
    ///
    /// assert_eq!(buf.len(), 3)
    /// ```
    #[inline]
    pub fn from_mut_slice(slice: &mut [S]) -> &Self {
        Self::from_slice_of_cells(Cell::from_mut(slice).as_slice_of_cells())
    }

    /// Creates a buffer reference from a shared reference to a slice of cells of samples.
    ///
    /// This is an alternative to [`AudioBuffer::from_mut_slice`] that allows to obtain an
    /// [`AudioBuffer`] reference in case you do not have exclusive (`&mut`) access to the sample
    /// data.
    ///
    /// # Example
    ///
    /// ```
    /// use std::cell::Cell;
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// // This function does *not* have &mut access to the data.
    /// fn use_shared_buffer(data: &[Cell<f32>]) {
    ///     let buf = AudioBuffer::from_slice_of_cells(data);
    ///     buf.fill(42.0)
    /// }
    ///
    /// let mut data = [0.0f32, 1.0, 2.0];
    /// let shareable: &[Cell<f32>] = Cell::from_mut(&mut data[..]).as_slice_of_cells();
    ///
    /// // Use or store a copy of the reference to the data.
    /// let shared_copy: &[Cell<f32>] = shareable;
    ///
    /// use_shared_buffer(shareable);
    ///
    /// assert_eq!(data, [42.0; 3])
    /// ```
    #[inline]
    #[must_use]
    pub const fn from_slice_of_cells(slice: &[Cell<S>]) -> &Self {
        // SAFETY: This type is repr(transparent), so the two types have the same memory layout
        unsafe { &*(slice as *const [Cell<S>] as *const Self) }
    }

    /// Forms a slice from a pointer and a length.
    ///
    /// The `len` argument is the number of *samples*, not the number of bytes.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if any of the following conditions are violated:
    ///
    /// * `data` must be [valid] for both reads *and* writes for `len * mem::size_of::<T>()` many bytes,
    ///   and it must be properly aligned. This means in particular:
    ///
    ///     * The entire memory range of this slice must be contained within a single allocated object!
    ///       Slices can never span across multiple allocated objects. See [below](#incorrect-usage)
    ///       for an example incorrectly not taking this into account.
    ///     * `data` must be non-null and aligned even for zero-length slices. One
    ///       reason for this is that enum layout optimizations may rely on references
    ///       (including slices of any length) being aligned and non-null to distinguish
    ///       them from other data. You can obtain a pointer that is usable as `data`
    ///       for zero-length slices using [`NonNull::dangling()`].
    ///
    /// * `data` must point to `len` consecutive properly initialized values of type `T`.
    ///
    /// * The total size `len * mem::size_of::<T>()` of the slice must be no larger than `isize::MAX`,
    ///   and adding that size to `data` must not "wrap around" the address space.
    ///   See the safety documentation of [`pointer::offset`].
    ///
    /// Note that unlike with the similar [`slice::from_raw_parts`] function, the memory referenced
    /// by the returned buffer *can* be mutated during the lifetime `'a`.
    ///
    /// # Caveat
    ///
    /// The lifetime for the returned slice is inferred from its usage. To
    /// prevent accidental misuse, it's suggested to tie the lifetime to whichever
    /// source lifetime is safe in the context, such as by providing a helper
    /// function taking the lifetime of a host value for the slice, or by explicit
    /// annotation.
    ///
    /// [valid]: ptr#safety
    /// [`pointer::offset`]: ptr::NonNull::offset
    /// [`NonNull::dangling()`]: ptr::NonNull::dangling
    /// [`slice::from_raw_parts`]: core::slice::from_raw_parts
    #[inline]
    pub unsafe fn from_raw_parts<'a>(ptr: *mut S, len: usize) -> &'a Self {
        if ptr.is_null() {
            null_audio_buffer()
        };

        Self::from_slice_of_cells(core::slice::from_raw_parts(ptr.cast(), len))
    }

    /// Returns the number of samples in the buffer.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the buffer is empty (i.e. its [`len`] is `0`), `false` otherwise.
    ///
    /// [`len`]: AudioBuffer::len
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns a raw pointer to the sample buffer.
    ///
    /// The resulting pointer can be used for both reading and writing sample data from/to the buffer.
    ///
    /// The caller must ensure that the slice outlives the pointer this function returns, or else
    /// it will end up dangling.
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// let mut data = [1.0, 2.0, 3.0];
    /// let buf: &AudioBuffer<f32> = AudioBuffer::from_mut_slice(&mut data);
    ///
    /// let buf_ptr = buf.as_ptr();
    ///
    /// unsafe {
    ///     for i in 0..buf.len() {
    ///         assert_eq!(buf.get(i), *buf_ptr.add(i)); // Read
    ///         buf_ptr.add(i).write(42.0); // Write
    ///     }
    /// }
    ///
    /// assert_eq!(buf, &[42.0; 3])
    /// ```
    #[inline]
    pub const fn as_ptr(&self) -> *mut S {
        self.inner.as_ptr().cast_mut().cast()
    }

    /// Returns this buffer as a slice of cells.
    #[inline]
    pub fn as_slice_of_cells(&self) -> &[Cell<S>] {
        &self.inner
    }

    /// Returns an iterator over the buffer.
    ///
    /// The iterator yields reference to [`Cell`s](Cell) of the samples, which allows for both
    /// reading and writing the samples.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    /// let mut data = [1.0; 3];
    ///
    /// let buf: &AudioBuffer<f32> = AudioBuffer::from_mut_slice(&mut data);
    ///
    /// for sample in buf { // sample is &Cell<f32>
    ///     assert_eq!(sample.get(), 1.0);
    ///     sample.set(42.0);
    /// }
    ///
    /// assert_eq!(data, [42.0; 3])
    ///
    /// ```
    #[inline]
    pub fn iter(&self) -> AudioBufferIter<S> {
        AudioBufferIter {
            inner: self.inner.iter(),
        }
    }

    #[inline]
    fn re_slice<I: SliceIndex<[Cell<S>], Output = [Cell<S>]>>(&self, index: I) -> &Self {
        AudioBuffer::from_slice_of_cells(self.inner.get(index).unwrap_or(&[]))
    }
}

impl<S: Copy> AudioBuffer<S> {
    /// Returns the value of the sample at the given `index` in the buffer.
    ///
    /// # Panics
    ///
    /// This method will panic if `index` is out of bounds.
    /// See [`try_get`](Self::try_get) for a non-panicking version of this method.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// let mut data = [0.0, 1.0, 2.0];
    /// let buf = AudioBuffer::from_mut_slice(&mut data);
    ///
    /// assert_eq!(buf.get(0), 0.0);
    /// assert_eq!(buf.get(1), 1.0);
    /// assert_eq!(buf.get(2), 2.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> S {
        self.inner[index].get()
    }

    /// Writes the given sample `value` at the given `index` in the buffer.
    ///
    /// # Panics
    ///
    /// This method will panic if `index` is out of bounds.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// let mut data = [0.0, 1.0, 2.0];
    /// let buf = AudioBuffer::from_mut_slice(&mut data);
    ///
    /// buf.put(1, 42.0);
    ///
    /// assert_eq!(data, [0.0, 42.0, 2.0]);
    /// ```
    #[inline]
    pub fn put(&self, index: usize, value: S) {
        self.inner[index].set(value)
    }

    /// Returns the value of the sample at the given `index` in the buffer, or `None` if `index` is
    /// out of bounds.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// let mut data = [0.0, 1.0, 2.0];
    /// let buf = AudioBuffer::from_mut_slice(&mut data);
    ///
    /// assert_eq!(buf.try_get(0), Some(0.0));
    /// assert_eq!(buf.try_get(1), Some(1.0));
    /// assert_eq!(buf.try_get(2), Some(2.0));
    /// assert_eq!(buf.try_get(3), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn try_get(&self, index: usize) -> Option<S> {
        Some(self.inner.get(index)?.get())
    }

    /// Returns the value of the sample at the given `index` in the buffer, without doing bounds
    /// checking.
    ///
    /// For a safe alternative, see [`get`](Self::get).
    ///
    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *undefined behavior*.
    #[inline]
    #[must_use]
    pub unsafe fn get_unchecked(&self, index: usize) -> S {
        self.inner.get_unchecked(index).get()
    }

    /// Writes the given sample `value` at the given `index` in the buffer, without doing bounds
    /// checking.
    ///
    /// For a safe alternative, see [`put`](Self::put).
    ///
    /// # Safety
    ///
    /// Calling this method with an out-of-bounds index is *undefined behavior*.
    #[inline]
    pub unsafe fn put_unchecked(&self, index: usize, value: S) {
        self.inner.get_unchecked(index).set(value)
    }

    /// Copies all the samples in this buffer into the given `buf` slice.
    ///
    /// # Panics
    ///
    /// This function will panic if the buffer and the `buf` slice have different lengths.
    #[inline]
    pub fn copy_to_slice(&self, buf: &mut [S]) {
        if buf.len() != self.len() {
            slice_len_mismatch(self.len(), buf.len())
        }

        // SAFETY: buf is guaranteed to be valid for writes, and this type guarantees the buffer
        // is valid for both reads and writes.
        // Buf was checked above to have the same length as this buffer.
        // Both are guaranteed to be properly aligned, since they are slices already.
        // Buf cannot overlap with this buffer, as it is behind an exclusive mutable reference.
        unsafe { ptr::copy_nonoverlapping(self.as_ptr(), buf.as_mut_ptr(), buf.len()) }
    }

    /// Copies all the samples in this buffer into a different buffer.
    ///
    /// # Panics
    ///
    /// This function will panic if the two buffers have different lengths.
    #[inline]
    pub fn copy_to_buffer(&self, buf: &AudioBuffer<S>) {
        buf.copy_from_buffer(self)
    }

    /// Copies all the samples in the given `buf` slice into this buffer.
    ///
    /// # Panics
    ///
    /// This function will panic if the buffer and the `buf` slice have different lengths.
    #[inline]
    pub fn copy_from_slice(&self, buf: &[S]) {
        if buf.len() != self.len() {
            slice_len_mismatch(buf.len(), self.len())
        }

        // SAFETY: buf is guaranteed to be valid for reads, and this type guarantees the buffer
        // is valid for both reads and writes.
        // Buf was checked above to have the same length as this buffer.
        // Both are guaranteed to be properly aligned, since they are slices already.
        // Buf cannot overlap with this buffer, as it is behind a shared immutable reference.
        unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), self.as_ptr(), buf.len()) }
    }

    /// Copies all the samples from another buffer into this buffer.
    ///
    /// # Panics
    ///
    /// This function will panic if the two buffers have different lengths.
    #[inline]
    pub fn copy_from_buffer(&self, buf: &AudioBuffer<S>) {
        if buf.len() != self.len() {
            slice_len_mismatch(buf.len(), self.len())
        }

        // SAFETY: This type guarantees the buffer are aligned and valid for both reads and writes.
        // Buf was checked above to have the same length as this buffer.
        unsafe { ptr::copy(buf.as_ptr(), self.as_ptr(), buf.len()) }
    }

    /// Fills the buffer with the given value.
    ///
    /// # Example
    ///
    /// ```
    /// use clack_plugin::prelude::AudioBuffer;
    ///
    /// let mut data = [0.0, 1.0, 2.0];
    /// let buf = AudioBuffer::from_mut_slice(&mut data);
    ///
    /// buf.fill(42.0);
    ///
    /// assert_eq!(data, [42.0; 3]);
    /// ```
    #[inline]
    pub fn fill(&self, value: S) {
        for i in &self.inner {
            i.set(value)
        }
    }
}

/// An iterator over the samples in an [`AudioBuffer`].
#[derive(Clone)]
pub struct AudioBufferIter<'a, S> {
    inner: core::slice::Iter<'a, Cell<S>>,
}

impl<'a, S> Iterator for AudioBufferIter<'a, S> {
    type Item = &'a Cell<S>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl<'a, S> IntoIterator for &'a AudioBuffer<S> {
    type Item = &'a Cell<S>;
    type IntoIter = AudioBufferIter<'a, S>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<S> Index<usize> for AudioBuffer<S> {
    type Output = Cell<S>;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.inner[index]
    }
}
impl<S> Index<core::ops::RangeFull> for AudioBuffer<S> {
    type Output = Self;

    #[inline]
    fn index(&self, index: core::ops::RangeFull) -> &Self::Output {
        self.re_slice(index)
    }
}
impl<S> Index<core::ops::Range<usize>> for AudioBuffer<S> {
    type Output = Self;

    #[inline]
    fn index(&self, index: core::ops::Range<usize>) -> &Self::Output {
        self.re_slice(index)
    }
}
impl<S> Index<core::ops::RangeFrom<usize>> for AudioBuffer<S> {
    type Output = Self;

    #[inline]
    fn index(&self, index: core::ops::RangeFrom<usize>) -> &Self::Output {
        self.re_slice(index)
    }
}
impl<S> Index<core::ops::RangeTo<usize>> for AudioBuffer<S> {
    type Output = Self;

    #[inline]
    fn index(&self, index: core::ops::RangeTo<usize>) -> &Self::Output {
        self.re_slice(index)
    }
}
impl<S> Index<core::ops::RangeInclusive<usize>> for AudioBuffer<S> {
    type Output = Self;

    #[inline]
    fn index(&self, index: core::ops::RangeInclusive<usize>) -> &Self::Output {
        self.re_slice(index)
    }
}
impl<S> Index<core::ops::RangeToInclusive<usize>> for AudioBuffer<S> {
    type Output = Self;

    #[inline]
    fn index(&self, index: core::ops::RangeToInclusive<usize>) -> &Self::Output {
        self.re_slice(index)
    }
}
impl<S> Index<(core::ops::Bound<usize>, core::ops::Bound<usize>)> for AudioBuffer<S> {
    type Output = Self;

    #[inline]
    fn index(&self, index: (core::ops::Bound<usize>, core::ops::Bound<usize>)) -> &Self::Output {
        self.re_slice(index)
    }
}

impl<S: Debug + Copy> Debug for AudioBuffer<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_list();
        for s in self {
            dbg.entry(&s.get());
        }
        dbg.finish()
    }
}

impl<S: PartialEq + Copy> PartialEq for AudioBuffer<S> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq(&other.inner)
    }
}

impl<S: PartialEq + Copy> PartialEq<[S]> for AudioBuffer<S> {
    fn eq(&self, other: &[S]) -> bool {
        if self.len() != other.len() {
            return false;
        }

        for (a, b) in self.iter().zip(other) {
            if a.get() != *b {
                return false;
            }
        }

        true
    }
}

impl<S: PartialEq + Copy, const N: usize> PartialEq<[S; N]> for AudioBuffer<S> {
    #[inline]
    fn eq(&self, other: &[S; N]) -> bool {
        PartialEq::<[S]>::eq(self, other)
    }
}

impl<S: PartialEq + Copy> PartialEq<&[S]> for AudioBuffer<S> {
    #[inline]
    fn eq(&self, other: &&[S]) -> bool {
        PartialEq::<[S]>::eq(self, other)
    }
}

impl<S: PartialEq + Copy, const N: usize> PartialEq<&[S; N]> for AudioBuffer<S> {
    #[inline]
    fn eq(&self, other: &&[S; N]) -> bool {
        PartialEq::<[S]>::eq(self, *other)
    }
}

impl<'a, S> From<&'a mut [S]> for &'a AudioBuffer<S> {
    #[inline]
    fn from(value: &mut [S]) -> &AudioBuffer<S> {
        AudioBuffer::from_mut_slice(value)
    }
}

impl<'a, S> From<&'a [Cell<S>]> for &'a AudioBuffer<S> {
    #[inline]
    fn from(value: &[Cell<S>]) -> &AudioBuffer<S> {
        AudioBuffer::from_slice_of_cells(value)
    }
}

#[cold]
#[inline(never)]
fn null_audio_buffer() -> ! {
    panic!("Invalid audio buffer: buffer pointer is NULL.")
}

#[cold]
#[inline(never)]
fn slice_len_mismatch(src_len: usize, dst_len: usize) -> ! {
    panic!(
        "Buffer size mismatch: source has length {}, but destination has length {}",
        src_len, dst_len
    )
}
