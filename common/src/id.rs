use core::num::NonZeroU32;

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub struct ClapId(NonZeroU32);

impl ClapId {
    #[inline]
    pub const fn new(raw: u32) -> Option<Self> {
        match NonZeroU32::new(raw.wrapping_add(1)) {
            Some(x) => Some(Self(x)),
            None => None,
        }
    }

    #[inline]
    pub const fn get(self) -> u32 {
        self.0.get().wrapping_sub(1)
    }
}
