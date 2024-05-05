//! Mini UART driver.

use core::fmt::{Result, Write};
use core::hint::spin_loop;

/// Base of the auxiliary peripheral configuration registers
const AUX_BASE: usize = 0x7E215000;
/// Auxiliary peripheral enabler register.
const AUX_ENABLES: *mut u32 = (AUX_BASE + 0x4) as _;
/// Input / output Mini UART register.
const AUX_MU_IO: *mut u32 = (AUX_BASE + 0x40) as _;
/// Data status Mini UART register.
const AUX_MU_LCR: *mut u32 = (AUX_BASE + 0x4C) as _;
/// Control MiniUART register.
const AUX_MU_CNTL: *mut u32 = (AUX_BASE + 0x60) as _;
/// Mini UART status register.
const AUX_MU_STAT: *const u32 = (AUX_BASE + 0x64) as _;
/// Mini UART BAUD rate divisor.
const AUX_MU_BAUD: *mut u32 = (AUX_BASE + 0x68) as _;
/// Base address of the GPIO registers.
const GPIO_BASE: usize = 0x7E200000;
/// GPIO function selection register 1.
const GPIO_FSEL1: *mut u32 = (GPIO_BASE + 0x4) as _;
/// GPIO pull-up / pull-down register 0.
const GPIO_PUPD0: *mut u32 = (GPIO_BASE + 0xE4) as _;
/// CPU frequency.
const GPU_FREQ: u32 = 500000000;
/// Desired BAUD rate.
const BAUD_RATE: u32 = 115200;

/// Whether the driver has been initialized.
static mut INIT: bool = false;

/// Send formatted diagnostic messages over the Mini UART.
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        write!($crate::uart::Uart, $($arg)*).unwrap();
        $crate::uart::Uart.write_str("\r\n").unwrap();
    }};
}

/// Send formatted diagnostic messages over the MiniUART without the trailing
/// CrLf.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        write!($crate::uart::Uart, $($arg)*).unwrap();
    }};
}

/// Mini UART driver.
pub struct Uart;

impl Uart
{
    fn init_or_nop()
    {
        unsafe {
            if INIT {
                return;
            }
            AUX_ENABLES.write_volatile(0x1);
            AUX_MU_CNTL.write_volatile(0x0);
            let val = GPIO_FSEL1.read_volatile();
            GPIO_FSEL1.write_volatile(val & 0xFFFC0FFF | 0x12000);
            let val = GPIO_PUPD0.read_volatile();
            GPIO_PUPD0.write_volatile(val & 0xFFFFFF);
            AUX_MU_LCR.write_volatile(0x3);
            AUX_MU_BAUD.write_volatile(GPU_FREQ / BAUD_RATE / 8 - 1);
            AUX_MU_CNTL.write_volatile(0x3);
            INIT = true;
        }
    }
}

impl Write for Uart
{
    fn write_str(&mut self, msg: &str) -> Result
    {
        Self::init_or_nop();
        for byte in msg.as_bytes() {
            while unsafe { AUX_MU_STAT.read_volatile() } & 0x20 != 0 {
                spin_loop()
            } // FIFO full.
            unsafe { AUX_MU_IO.write_volatile(*byte as _) };
        }
        Ok(())
    }
}
