//! Direct memory access controller driver.
//!
//! Implements a simple DMA driver that reads cyclically from a single buffer
//! and sends the data to a peripheral.

use core::marker::PhantomPinned;
use core::mem::size_of_val;
use core::sync::atomic::{fence, Ordering};

use crate::vcalloc::alloc;
use crate::{mbox, println};

/// Get available DMA channels tag.
const GET_DMA_TAG: u32 = 0x60001;
/// Normal DMA channels mask.
const NORMAL_MASK: u32 = 0x7F;
/// Total number of DMA channels.
const CHAN_COUNT: u32 = 16;
/// Base address.
const BASE: usize = 0x7E007000;
/// Channel 0 control and status register.
const CH0_CS: *mut u32 = BASE as _;
/// Channel 0 control block register.
const CH0_CB: *mut u32 = (BASE + 0x4) as _;
/// Channel address stride.
const STRIDE: usize = 0x100;

/// Control block.
#[repr(align(32), C)]
#[derive(Debug)]
struct ControlBlock
{
    /// Transfer information.
    ti: u32,
    /// Source address.
    src: u32,
    /// Destination address.
    dst: u32,
    /// Length in bytes.
    len: u32,
    /// Stride for 2D access.
    stride: u32,
    /// Next control block address.
    next: u32,
    /// Padding.
    _pad: [u32; 2],
    /// Pinning.
    _pin: PhantomPinned,
}

// Sets up a DMA channel to repeatedly send data to a peripheral.
pub unsafe fn setup_sender<T>(src: &[T], dst: *mut u32, dreq: u32)
{
    let cb = alloc::<ControlBlock>();
    let dreq = dreq & 0x1F;
    *cb = ControlBlock { ti: 0x348 | (dreq << 16),
                         src: src.as_ptr() as usize as u32,
                         dst: dst as usize as u32,
                         len: size_of_val(src) as u32,
                         stride: 0,
                         next: cb as usize as u32,
                         _pad: [0; 2],
                         _pin: PhantomPinned };
    fence(Ordering::Release);
    let dma_out: u32;
    mbox! {GET_DMA_TAG: _ => dma_out};
    let avail = dma_out & NORMAL_MASK;
    let mut chan = CHAN_COUNT;
    for bit in 0 .. CHAN_COUNT {
        if avail & (1 << bit) != 0 {
            chan = bit;
            break;
        }
    }
    assert!(chan < CHAN_COUNT, "No available normal DMA channels");
    let offset = STRIDE / 4 * chan as usize;
    CH0_CS.add(offset).write_volatile(0x80000000);
    CH0_CB.add(offset).write_volatile(cb as usize as u32);
    CH0_CS.add(offset).write_volatile(0x20A50007);
    println!("Initialized DMA channel #{chan}");
}
