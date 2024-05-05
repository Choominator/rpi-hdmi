//! Video Core memory allocator.
//!
//! Uses the Video Core's firmware to dynamically allocate memory. for DMA
//! transfers, since we aren't implementing an actual allocator and DMA buffers
//! cover all our needs.

use core::alloc::Layout;

use crate::mbox;

/// Allocate memory property tag.
const ALLOC_MEM_TAG: u32 = 0x3000C;
/// Lock memory property tag.
const LOCK_MEM_TAG: u32 = 0x3000D;

/// Allocate memory property input.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct AllocMemoryPropertyInput
{
    /// Size of allocation.
    size: u32,
    /// Alignment of allocation.
    align: u32,
    /// Allocation Fflags.
    flags: u32,
}

/// Allocates uncached memory suitable for DMA transfers.
pub fn alloc<T>() -> *mut T
{
    let layout = Layout::new::<T>();
    let alloc_in = AllocMemoryPropertyInput { size: layout.size() as u32,
                                              align: layout.align() as u32,
                                              flags: 0x54 /* Permanente, uncached, zero-filled. */ };
    let alloc_out: u32;
    mbox! {ALLOC_MEM_TAG: alloc_in => alloc_out};
    assert!(alloc_out > 0, "Failed to allocate VC memory");
    let lock_in = alloc_out;
    let lock_out: u32;
    mbox! {LOCK_MEM_TAG: lock_in => lock_out};
    assert!(lock_out > 0, "Failed to lock VC memory");
    lock_out as usize as *mut T
}
