use core::hint::spin_loop;
use core::sync::atomic::{fence, Ordering};

use crate::dma::setup_sender;
use crate::scalloc::alloc;
use crate::{mbox, println};

/// Core register block base.
const BASE: usize = 0x107C701400;
/// Audio channel map.
const AU_CHMAP: *mut u32 = (BASE + 0xA4) as _;
/// Audio configuration register.
const AU_CFG: *mut u32 = (BASE + 0xA8) as _;
/// Packet configuration register.
const AU_PKTCFG: *mut u32 = (BASE + 0xC0) as _;
/// Info frame configuration register.
const IF_CFG: *mut u32 = (BASE + 0xC4) as _;
/// Info frame status register.
const IF_STATUS: *mut u32 = (BASE + 0xCC) as _;
/// Content type reporting packet configuration register.
const CRP_CFG: *mut u32 = (BASE + 0xD0) as _;
/// Clock to service register 0.
const CTS0: *mut u32 = (BASE + 0xD4) as _;
/// Clock to service register 1.
const CTS1: *mut u32 = (BASE + 0xD8) as _;
/// Info frame packet register block base.
const IF_BASE: usize = 0x107C703800;
/// First register of the info frame packet block.
const IF_START: *mut u32 = IF_BASE as _;
/// HD register block base.
const HD_BASE: usize = 0x107C720000;
/// HD audio control register.
const HD_AU_CTL: *mut u32 = (HD_BASE + 0x10) as _;
/// HD audio DMA DREQ thresholds configuration register.
const HD_AU_THR: *mut u32 = (HD_BASE + 0x14) as _;
/// HD audio format register.
const HD_AU_FMT: *mut u32 = (HD_BASE + 0x18) as _;
/// HD audio data FIFO register.
const HD_AU_DATA: *mut u32 = (HD_BASE + 0x1C) as _;
/// HD audio clock division register.
const HD_AU_SMP: *mut u32 = (HD_BASE + 0x20) as _;
/// CPRMAN clock rate.
const CLOCK_FREQ: u32 = 54000000;
/// Pixel clock rate.
const PIXCLOCK_FREQ: u32 = 148500000;
/// Audio sample rate.
const SAMPLE_RATE: u32 = 48000;
/// Data request device ID.
const DREQ: u32 = 10;
/// Audio buffer length in words (must fit in a 128KB buffer).
const AU_BUF_LEN: usize = (SAMPLE_RATE * 2 / 4) as _;
/// Get frame buffer memory property tag.
const GET_FB_TAG: u32 = 0x40001;
/// Get frame buffer depth tag.
const GET_FB_DEPTH_TAG: u32 = 0x40005;

// Generates a value with the specified bit fields.
macro_rules! bits {
    {$start:literal ..= $end:literal => $val:expr $(,)?} => {{
        assert!($val < 1 << ($end - $start + 1), "Value 0x{:X} does not fit in a {} bit wide field", $val, $end - $start + 1);
        $val << $start
    }};
    {$bit:literal => $val:expr $(,)?} => {{
        assert!($val < 2, "Value {:X} does not fit in a 1 bit wide field", $val);
        $val << $bit
    }};
    {$start:literal $(..= $end:literal)? => $val:expr,
        $($startargs:literal $(..= $endargs:literal)? => $valargs:expr),+ $(,)?} => {{
        bits!($start $(..= $end)? => $val) | bits!($($startargs $(..= $endargs)? => $valargs),+)
    }};
}

/// Sets up the HDMI controller to output video and audio.
#[track_caller]
pub fn init()
{
    let get_fb_in: u32 = 4;
    let get_fb_out: [u32; 2];
    let get_fb_depth_out: u32;
    mbox! {
        GET_FB_TAG: get_fb_in => get_fb_out,
        GET_FB_DEPTH_TAG: _ => get_fb_depth_out,
    };
    if get_fb_depth_out == 16 {
        let fb = get_fb_out[0] as usize as *mut u16;
        for idx in 0 .. get_fb_out[1] as usize / 2 {
            unsafe {
                fb.add(idx).write(0x07E0);
            }
        }
    } else if get_fb_depth_out == 32 {
        let fb = get_fb_out[0] as usize as *mut u32;
        for idx in 0 .. get_fb_out[1] as usize / 4 {
            unsafe {
                fb.add(idx).write(0xFF00FF00);
            }
        }
    } else {
        println!("Unsupported pixel depth: {get_fb_depth_out}");
    }
    // Wait for the video core to prepare the HDMI registers.
    for _ in 0 .. 1000000 {
        spin_loop()
    }
    println!("Video initialized");
    let abuf = alloc::<[u32; AU_BUF_LEN]>();
    synthesize(unsafe { &mut *abuf });
    unsafe {
        let hd_au_ctl = bits! {
            // Clear starvation bit.
            15 => 1,
            // Not sure what this does, but it's set on Linux.
            13 => 1,
            // Not sure what this does, but it's set on Linux.
            12 => 1,
            // Compute parity bits for IEC958 subframes.
            8 => 1,
            // Channel count.
            4 ..= 7 => 2,
            // Enable HDMI audio.
            3 => 1,
            // Clear underflow error bit.
            2 => 1,
            // Clear overflow error bit.
            1 => 1,
        };
        HD_AU_CTL.write_volatile(hd_au_ctl);
        // Info frames must only b updated when disabled with their register block
        // enabled.
        let ifcfg = IF_CFG.read_volatile();
        let ifcfgset = bits! {
            // Enable info frame register block.
            16 => 1,
        };
        let ifcfgclr = bits! {
            // Audio info frame.
            4 => 1,
        };
        IF_CFG.write_volatile(ifcfg & !ifcfgclr | ifcfgset);
        while IF_STATUS.read_volatile() & ifcfgclr != 0 {
            spin_loop();
        }
        // Audio info frame offset (info frame 4, register stride 9).
        let offset = 4 * 9;
        let if40 = bits! {
            // Info frame length.
            16 ..= 23 => 10,
            // Info frame version.
            8 ..= 15 => 1,
            // Info frame type (audio).
            0 ..= 7 => 0x84,
        };
        IF_START.add(offset).write_volatile(if40);
        let if41 = bits! {
            // Allocate channel 1.
            25 => 1,
            // Allocate channel 0.
            24 => 1,
            // Sample rate (48000Hz).
            10 ..= 12 => 3,
            // Sample size (16 bit).
            8 ..= 9 => 1,
            // Coding type (PCM).
            4 ..= 7 => 1,
            // Last channel index.
            0 ..= 2 => 1,
        };
        IF_START.add(offset + 1).write_volatile(if41);
        // The hardware computes the info frame checksums using their whole register
        // blocks, so all the remaining unused registers must be zeroed.
        for idx in 2 .. 9 {
            IF_START.add(offset + idx).write_volatile(0);
        }
        let ifcfgset = ifcfgclr;
        IF_CFG.write_volatile(ifcfg | ifcfgset);
        let au_cfg = bits! {
            // Not sure what this does, but Linux sets it.
            27 => 1,
            // Not sure what this does, but Linux sets it.
            26 => 1,
            // Enable channel 1.
            1 => 1,
            // Enable channel 0.
            0 => 1,
        };
        AU_CFG.write_volatile(au_cfg);
        let au_pktcfg = bits! {
            // Zero data on flat sample.
            29 => 1,
            // Zero data on inactive channels.
            24 => 1,
            // B frame preamble.
            10 ..= 13 => 0x8,
            // Channel 1.
            1 => 1,
            // Channel 0.
            0 => 1,
        };
        AU_PKTCFG.write_volatile(au_pktcfg);
        let au_chmap = bits! {
            // Map channel 1 to channel 1.
            4 ..= 6 => 1,
            // Map channel 0 to channel 0.
            0 ..= 2 => 0,
        };
        AU_CHMAP.write_volatile(au_chmap);
        let hd_au_fmt = bits! {
            // Coding type (PCM).
            16 ..= 23 => 2,
            // Sample rate (48000).
            8 ..= 15 => 9,
        };
        HD_AU_FMT.write_volatile(hd_au_fmt);
        let hd_au_thr = bits! {
            // Set panic data request threshold.
            24 ..= 31 => 16,
            // Clear panic data request threshold.
            16 ..= 23 => 16,
            // Set normal data request threshold.
            8 ..= 15 => 28,
            // Clear normal data request threshold.
            0 ..= 7 => 28,
        };
        HD_AU_THR.write_volatile(hd_au_thr);
        let hd_au_smp = bits! {
            // Numerator.
            8 ..= 31 => CLOCK_FREQ / SAMPLE_RATE * 2,
            // Denominator (1).
            0 ..= 7 => 0,
        };
        HD_AU_SMP.write_volatile(hd_au_smp);
        // I don't know how to operate the following registers, so I'm setting them to
        // the same values as Linux does for the same audio and video configuration.
        CRP_CFG.write_volatile(0x1000000 | (SAMPLE_RATE * 128 / 1000));
        CTS0.write_volatile(PIXCLOCK_FREQ / 1000);
        CTS1.write_volatile(PIXCLOCK_FREQ / 1000);
        println!("Audio initialized");
        setup_sender(&*abuf, HD_AU_DATA, DREQ);
    }
}

/// Synthesizes two audio tones in different frequencies to each of the stereo
/// channels.
fn synthesize(buf: &mut [u32; AU_BUF_LEN])
{
    // 192 channel status bits.
    let mut cs = [0; 24];
    let csconf = [0x4,  // SPDIF, PCM, no copyright, no emphasis.
                  0x44, // Software broadcast.
                  0x0,  // Channel (to fill in later).
                  0x2,  // 48000Hz, 1000ppm.
                  0xD2  /* 16 bit sample size, 48000Hz original frequency. */];
    cs[0 .. 5].copy_from_slice(&csconf);
    for (idx, output) in buf.iter_mut().enumerate() {
        // We're dealing with twice as many frames here since we are synthesizing for
        // two channels one at a time, so the math must take that into account.
        let halfperiod = if idx & 0x1 == 0 {
            SAMPLE_RATE as usize / 200
        } else {
            SAMPLE_RATE as usize / 300
        };
        let sample = if (idx / halfperiod) & 0x1 == 1 {
            // Positive phase.
            0x3FFF
        } else {
            // Negative phase.
            0x0C000
        };
        let blockidx = idx % (192 * 2);
        // Mark B subframes according to the configuration in the AU_PKTCFG register.
        let preamble = ((blockidx == 0) as u32) << 3;
        let byte = blockidx >> 4;
        let bit = (blockidx >> 1) & 0x7;
        let cs = if blockidx == 16 * 2 + 1 || blockidx == 20 * 2 + 1 {
            // Nibbles 4 and 5 of channel status contain the source and destination channel
            // indices. Channel 0 has index 0 so nothing needs to be done, but channel 1
            // must have its index bits set appropriately.
            0x1
        } else {
            (cs[byte] >> bit) & 0x1
        };
        *output = bits! {
            // 8 * 24 channel status bits spread across 192 subframes per channel.
            30 => cs,
            // Signed 16 bit audio sample.
            12 ..= 27 => sample,
            // Preamble.
            0 ..= 3 => preamble,
        };
    }
    fence(Ordering::Release);
}
