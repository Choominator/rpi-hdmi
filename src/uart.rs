//! AMBA PL011 UART driver.

use core::fmt::{Result, Write};
use core::hint::spin_loop;

/// Base address.
const BASE: usize = 0x107D001000;
/// Data FIFO register.
const DATA: *mut u32 = BASE as _;
/// Flags register.
const FLAGS: *mut u32 = (BASE + 0x18) as _;
/// Integer clock divisor.
const INT_DIV: *mut u32 = (BASE + 0x24) as _;
/// Fractional clock divisor.
const FRAC_DIV: *mut u32 = (BASE + 0x28) as _;
/// Control register.
const CTL: *mut u32 = (BASE + 0x30) as _;
/// Clock rate.
const CLOCK_RATE: u32 = 9216000;
/// Desired BAUD rate.
const BAUD_RATE: u32 = 115200;

/// Whether the driver has been initialized.
static mut INIT: bool = false;

/// Send formatted diagnostic messages over the UART.
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        write!($crate::uart::Uart, $($arg)*).unwrap();
        $crate::uart::Uart.write_str("\r\n").unwrap();
    }};
}

/// Send formatted diagnostic messages over the UART without the trailing
/// CrLf.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        write!($crate::uart::Uart, $($arg)*).unwrap();
    }};
}

/// AMBA PL011 UART driver.
pub struct Uart;

impl Uart
{
    fn init_or_nop()
    {
        unsafe {
            if INIT {
                return;
            }
            let quot = CLOCK_RATE * 8 / BAUD_RATE;
            INT_DIV.write_volatile(quot >> 6);
            FRAC_DIV.write_volatile(quot & 0x3F);
            CTL.write_volatile(0x101);
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
            while unsafe { FLAGS.read_volatile() } & 0x20 != 0 {
                spin_loop();
            }
            unsafe { DATA.write_volatile(*byte as _) };
        }
        Ok(())
    }
}
