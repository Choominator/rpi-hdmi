//! Scratch allocator.
//!
//! Allocates memory in an uncached region to communicate with peripherals.

use core::alloc::Layout;

/// Base address to the uncached memory region.
const UNCACHED_BASE: usize = 0x4000000;

/// Amount of memory allocated.
static mut SCRATCHED: usize = 0;

/// Allocates uncached memory for data of the specified type.
pub fn alloc<T>() -> *mut T
{
    unsafe {
        let layout = Layout::new::<T>();
        SCRATCHED = (SCRATCHED + layout.align() - 1) & !(layout.align() - 1);
        let base = (UNCACHED_BASE + SCRATCHED) as *mut T;
        SCRATCHED += layout.size();
        base
    }
}
