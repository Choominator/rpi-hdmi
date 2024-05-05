#![no_std]
#![no_main]
#![feature(panic_info_message)]

mod dma;
mod hdmi;
mod mbox;
mod uart;
mod vcalloc;

use core::arch::{asm, global_asm};
use core::fmt::Write;
use core::mem::size_of_val;
use core::panic::PanicInfo;
use core::sync::atomic::{compiler_fence, Ordering};

use self::uart::Uart;

/// Size of a cache line.
const CACHELINE_SIZE: usize = 64;

global_asm!(include_str!("boot.s"));

/// Entry point.
#[no_mangle]
pub extern "C" fn start() -> !
{
    println!("Starting");
    hdmi::init();
    halt()
}

/// Panics with diagnostic information about a fault.
#[no_mangle]
pub extern "C" fn fault(kind: usize) -> !
{
    let level: usize;
    let syndrome: usize;
    let addr: usize;
    let ret: usize;
    let state: usize;
    unsafe {
        asm!(
            "mrs {el}, currentel",
            "lsr {el}, {el}, #2",
            el = out (reg) level,
            options (nomem, nostack, preserves_flags));
        match level {
            2 => asm!(
                    "mrs {synd}, esr_el2",
                    "mrs {addr}, far_el2",
                    "mrs {ret}, elr_el2",
                    "mrs {state}, spsr_el2",
                    synd = out (reg) syndrome,
                    addr = out (reg) addr,
                    ret = out (reg) ret,
                    state = out (reg) state,
                    options (nomem, nostack, preserves_flags)),
            1 => asm!(
                    "mrs {synd}, esr_el1",
                    "mrs {addr}, far_el1",
                    "mrs {ret}, elr_el1",
                    "mrs {state}, spsr_el1",
                    synd = out (reg) syndrome,
                    addr = out (reg) addr,
                    ret = out (reg) ret,
                    state = out (reg) state,
                    options (nomem, nostack, preserves_flags)),
            _ => panic!("Exception caught at unsupported level {level}"),
        }
    };
    panic!("Triggered an exception at level {level}: Kind: 0x{kind:x}, Syndrome: 0x{syndrome:x}, Address: 0x{addr:x}, Location: 0x{ret:x}, State: 0x{state:x}");
}

/// Halts the system.
#[no_mangle]
pub extern "C" fn halt() -> !
{
    println!("Halted");
    unsafe {
        asm!("msr daifset, #0x3",
             "0:",
             "wfe",
             "b 0b",
             options(nomem, nostack, preserves_flags, noreturn))
    }
}

/// Halts the system with a diagnostic error message.
#[panic_handler]
fn panic(info: &PanicInfo) -> !
{
    if let Some(location) = info.location() {
        print!("Panicked at {}:{}: ", location.file(), location.line());
    } else {
        print!("Panic: ");
    }
    if let Some(args) = info.message() {
        Uart.write_fmt(*args).unwrap()
    } else {
        Uart.write_str("Unknown reason").unwrap()
    }
    Uart.write_str("\r\n").unwrap();
    halt();
}

/// Invalidates the cache associated with the specified data to point of
/// coherence, effectively purging the data object from cache without writing it
/// out to memory.  Other objects sharing the same initial or final cache lines
/// as the object being purged will have their contents restored at the end of
/// this operation.
///
/// * `data`: Data object to purge from cache.
pub fn invalidate_cache<T: Copy>(data: &mut T)
{
    let size = size_of_val(data);
    if size == 0 {
        return;
    }
    let start = data as *mut T as usize;
    let end = data as *mut T as usize + size;
    let algn_start = start & !(CACHELINE_SIZE - 1);
    let algn_end = (end + (CACHELINE_SIZE - 1)) & !(CACHELINE_SIZE - 1);
    // Save the first and last cache lines.
    let start_cl = unsafe { *(algn_start as *const [u8; CACHELINE_SIZE]) };
    let end_cl = unsafe { *((algn_end - CACHELINE_SIZE) as *const [u8; CACHELINE_SIZE]) };
    // Invalidate the cache.
    compiler_fence(Ordering::Release);
    unsafe { asm!("dsb sy", options(nomem, nostack, preserves_flags)) };
    for addr in (algn_start .. algn_end).step_by(CACHELINE_SIZE) {
        unsafe { asm!("dc ivac, {addr}", addr = in (reg) addr, options (preserves_flags)) };
    }
    unsafe { asm!("dsb sy", options(nomem, nostack, preserves_flags)) };
    compiler_fence(Ordering::Acquire);
    // Restore the parts of the first and last cachelines shared with this data
    // object.
    if algn_start != start {
        let count = start - algn_start;
        unsafe {
            (algn_start as *mut u8).copy_from_nonoverlapping(&start_cl[0], count);
        }
    }
    if algn_end != end {
        let count = algn_end - end;
        let idx = CACHELINE_SIZE - count;
        unsafe {
            (end as *mut u8).copy_from_nonoverlapping(&end_cl[idx], count);
        }
    }
}

/// Cleans up the cache associated with the specified data object, effectively
/// flushing its contents to main memory.
///
/// * `data`: Data object to flush.
pub fn cleanup_cache<T: Copy>(data: &T)
{
    let size = size_of_val(data);
    if size == 0 {
        return;
    }
    let start = data as *const T as usize & !(CACHELINE_SIZE - 1);
    let end = (data as *const T as usize + size + (CACHELINE_SIZE - 1)) & !(CACHELINE_SIZE - 1);
    compiler_fence(Ordering::Release);
    unsafe { asm!("dsb sy", options(nomem, nostack, preserves_flags)) };
    for addr in (start .. end).step_by(CACHELINE_SIZE) {
        unsafe { asm!("dc cvac, {addr}", addr = in (reg) addr, options (nomem, nostack, preserves_flags)) };
    }
    unsafe { asm!("dsb sy", options(nomem, nostack, preserves_flags)) };
}
