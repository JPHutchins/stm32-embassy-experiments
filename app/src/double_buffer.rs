use core::cell::UnsafeCell;
use core::fmt;
use core::ops::Deref;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct DoubleBuffer<const N: usize> {
    halves: UnsafeCell<[[u16; N]; 2]>,
    leased: [AtomicBool; 2],
}

unsafe impl<const N: usize> Sync for DoubleBuffer<N> {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Half {
    First,
    Second,
}

pub enum Completion<'a, const N: usize> {
    Ready(Lease<'a, N>),
    Overrun,
}

pub struct Lease<'a, const N: usize> {
    buffer: &'a DoubleBuffer<N>,
    half: Half,
}

impl<const N: usize> DoubleBuffer<N> {
    pub const LEN: usize = 2 * N;

    pub const fn new() -> Self {
        Self {
            halves: UnsafeCell::new([[0; N]; 2]),
            leased: [AtomicBool::new(false), AtomicBool::new(false)],
        }
    }

    pub const fn dma_target(&self) -> *mut u16 {
        self.halves.get().cast()
    }

    pub fn idle(&self) -> bool {
        !self.is_leased(Half::First) && !self.is_leased(Half::Second)
    }

    pub fn completed(&self, half: Half) -> Completion<'_, N> {
        match self.idle() {
            true => Completion::Ready(self.leased_out(half)),
            false => Completion::Overrun,
        }
    }

    fn leased_out(&self, half: Half) -> Lease<'_, N> {
        self.leased[half.index()].store(true, Ordering::Release);
        Lease { buffer: self, half }
    }

    fn is_leased(&self, half: Half) -> bool {
        self.leased[half.index()].load(Ordering::Acquire)
    }

    fn released(&self, half: Half) {
        self.leased[half.index()].store(false, Ordering::Release);
    }

    unsafe fn half(&self, half: Half) -> &[u16; N] {
        unsafe { &*self.halves.get().cast::<[u16; N]>().add(half.index()) }
    }
}

impl<const N: usize> Default for DoubleBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl Half {
    const fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}

impl<const N: usize> Deref for Lease<'_, N> {
    type Target = [u16];

    fn deref(&self) -> &[u16] {
        unsafe { self.buffer.half(self.half) }
    }
}

impl<const N: usize> Drop for Lease<'_, N> {
    fn drop(&mut self) {
        self.buffer.released(self.half);
    }
}

impl<const N: usize> fmt::Debug for Lease<'_, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Lease").field(&self.half).finish()
    }
}

#[cfg(feature = "defmt")]
impl<const N: usize> defmt::Format for Lease<'_, N> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "Lease({})", self.half)
    }
}

#[cfg(test)]
mod tests {
    use super::{Completion, DoubleBuffer, Half};

    #[test]
    fn a_completed_half_is_leased_until_the_lease_drops() {
        let buffer = DoubleBuffer::<4>::new();
        match buffer.completed(Half::First) {
            Completion::Ready(lease) => {
                assert_eq!(&*lease, &[0; 4]);
                assert!(!buffer.idle());
            }
            Completion::Overrun => panic!("a fresh buffer cannot overrun"),
        }
        assert!(buffer.idle());
    }

    #[test]
    fn the_dma_wrapping_onto_a_leased_half_is_an_overrun() {
        let buffer = DoubleBuffer::<4>::new();
        let held = buffer.completed(Half::First);
        assert!(matches!(
            buffer.completed(Half::Second),
            Completion::Overrun
        ));
        drop(held);
        assert!(matches!(
            buffer.completed(Half::Second),
            Completion::Ready(_)
        ));
    }

    #[test]
    fn completing_a_half_that_is_still_leased_is_an_overrun() {
        let buffer = DoubleBuffer::<4>::new();
        let _held = buffer.completed(Half::Second);
        assert!(matches!(
            buffer.completed(Half::Second),
            Completion::Overrun
        ));
    }

    #[test]
    fn a_lease_reads_what_the_dma_wrote_into_its_half() {
        let buffer = DoubleBuffer::<4>::new();
        (0..DoubleBuffer::<4>::LEN).for_each(|i| unsafe {
            buffer
                .dma_target()
                .add(i)
                .write(u16::try_from(i).expect("test index fits u16"))
        });
        match buffer.completed(Half::Second) {
            Completion::Ready(lease) => assert_eq!(&*lease, &[4, 5, 6, 7]),
            Completion::Overrun => panic!("a fresh buffer cannot overrun"),
        }
    }
}
