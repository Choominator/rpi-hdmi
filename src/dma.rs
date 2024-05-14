//! Direct memory access controller driver.
//!
//! Implements a simple DMA driver that reads cyclically from a single buffer
//! and sends the data to a peripheral.

use core::marker::PhantomPinned;
use core::mem::size_of_val;
use core::sync::atomic::{fence, Ordering};

use crate::println;
use crate::scalloc::alloc;

/// Base address.
const BASE: usize = 0x1000010000;
/// Channel 0 control and status register.
const CH0_CS: *mut u32 = BASE as _;
/// Channel 0 control block register.
const CH0_CB: *mut u32 = (BASE + 0x4) as _;

/// Control block.
#[repr(align(32), C)]
#[derive(Debug)]
struct ControlBlock
{
    /// Transfer information.
    ti: u32,
    /// Lower 32 bits of source address.
    src: u32,
    /// Lower 32 bits of destination address.
    dst: u32,
    /// Length in bytes.
    len: u32,
    /// High 8 bits of source and destination addresses.
    hisrcdst: u32,
    /// Next control block address shifted 5 bits to the right.
    next: u32,
    /// Padding.
    _pad: [u32; 2],
    /// Pinning.
    _pin: PhantomPinned,
}

// Sets up a DMA channel to repeatedly send data to a peripheral.
pub unsafe fn setup_sender<T>(src: &[T], dst: *mut u32, dreq: u32)
{
    let dreq = dreq & 0x1F;
    let cb0 = alloc::<ControlBlock>();
    let cb1 = alloc::<ControlBlock>();
    *cb0 = ControlBlock { ti: 0xF348 | (dreq << 16),
                          src: src.as_ptr() as usize as u32,
                          dst: (dst as usize & 0xFFFFFFFF) as u32,
                          len: size_of_val(src) as u32 / 2,
                          hisrcdst: ((dst as usize >> 24) as u32 & 0xFF00)
                                    | (src.as_ptr() as usize >> 32) as u32 & 0xFF,
                          next: (cb1 as usize >> 5) as u32,
                          _pad: [0; 2],
                          _pin: PhantomPinned };
    *cb1 = ControlBlock { ti: 0xF348 | (dreq << 16),
                          src: src.as_ptr() as usize as u32 + (*cb0).len,
                          dst: (dst as usize & 0xFFFFFFFF) as u32,
                          len: size_of_val(src) as u32 / 2,
                          hisrcdst: ((dst as usize >> 24) as u32 & 0xFF00)
                                    | (src.as_ptr() as usize >> 32) as u32 & 0xFF,
                          next: (cb0 as usize >> 5) as u32,
                          _pad: [0; 2],
                          _pin: PhantomPinned };
    fence(Ordering::Release);
    CH0_CS.write_volatile(0x80000000);
    CH0_CB.write_volatile(cb0 as usize as u32 >> 5);
    CH0_CS.write_volatile(0x20A50007);
    println!("Initialized DMA channel #0");
}
