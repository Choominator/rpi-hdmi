use core::hint::spin_loop;
use core::sync::atomic::{fence, Ordering};

use crate::dma::setup_sender;
use crate::vcalloc::alloc;
use crate::{mbox, println};

/// Core register block base.
const BASE: usize = 0x7EF00700;
/// Audio channel map.
const AU_CHMAP: *mut u32 = (BASE + 0x9C) as _;
/// Audio configuration register.
const AU_CFG: *mut u32 = (BASE + 0xA0) as _;
/// Packet configuration register.
const AU_PKTCFG: *mut u32 = (BASE + 0xB8) as _;
/// Info frame configuration register.
const IF_CFG: *mut u32 = (BASE + 0xBC) as _;
/// Info frame status register.
const IF_STATUS: *mut u32 = (BASE + 0xC4) as _;
const CRP_CFG: *mut u32 = (BASE + 0xC8) as _;
const CTS0: *mut u32 = (BASE + 0xCC) as _;
const CTS1: *mut u32 = (BASE + 0xD0) as _;
/// Info frame packet register block base.
const IF_BASE: usize = 0x7EF01B00;
/// First register of the info frame packet block.
const IF_START: *mut u32 = IF_BASE as _;
/// HD register block base.
const HD_BASE: usize = 0x7EF20000;
/// HD audio control register.
const HD_AU_CTL: *mut u32 = (HD_BASE + 0x10) as _;
/// HD audio DMA DREQ thresholds configuration register.
const HD_AU_THR: *mut u32 = (HD_BASE + 0x14) as _;
/// HD audio format register.
const HD_AU_FMT: *mut u32 = (HD_BASE + 0x18) as _;
/// HD audio data register.
const HD_AU_DATA: *mut u32 = (HD_BASE + 0x1C) as _;
/// HD audio clock division register.
const HD_AU_SMP: *mut u32 = (HD_BASE + 0x20) as _;
/// Screen width in pixels.
const SCREEN_WIDTH: usize = 1920;
/// Screen height in pixels.
const SCREEN_HEIGHT: usize = 1080;
/// Pixel depth in bytes.
const DEPTH: usize = 4;
/// Horizontal pitch in bytes.
const PITCH: usize = SCREEN_WIDTH * DEPTH;
/// Vertical pitch in rows.
const VPITCH: usize = 1;
/// Set plane property tag.
const SET_PLANE_TAG: u32 = 0x48015;
/// Display ID.
const DISP_ID: u8 = 2;
/// Plane image type XRGB8888 setting.
const IMG_XRGB8888_TYPE: u8 = 44;
/// CPRMAN clock rate.
const CLOCK_RATE: u32 = 54000000;
/// Pixel clock rate in milliseconds.
const PIXCLOCK_RATE: u32 = 148500;
/// Audio sample rate.
const SAMPLE_RATE: u32 = 48000;
/// Data request device ID.
const DREQ: u32 = 10;
/// Video buffer length in words.
const VID_BUF_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT;
/// Audio buffer length in words.
const AU_BUF_LEN: usize = (SAMPLE_RATE * 2) as _;

/// Set plane property.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct SetPlanePropertyInput
{
    // Display ID.
    display_id: u8,
    /// Plane ID.
    plane_id: u8,
    /// Image type.
    img_type: u8,
    /// Display layer.
    layer: i8,
    /// Physical width.
    width: u16,
    /// Physical height.
    height: u16,
    /// Physical horizontal pitch (in bytes).
    pitch: u16,
    /// Physical vertical pitch (in rows).
    vpitch: u16,
    /// Horizontal offset into the source image (16.16 fixed point).
    src_x: u32,
    /// Vertical offset into the source image (16.16 fixed point).
    src_y: u32,
    /// Width of the source image (16.16 fixed point).
    src_w: u32,
    /// Height of the source image (16.16 fixed point).
    src_h: u32,
    /// Horizontal offset into the destination image.
    dst_x: i16,
    /// Vertical offset into the destination image.
    dst_y: i16,
    /// Width of the destination image.
    dst_w: u16,
    /// Height of the destination image.
    dst_h: u16,
    /// Opacity.
    alpha: u8,
    /// Number of subplanes comprising this plane (always 1 as other subplanes
    /// are used for composite formats).
    num_planes: u8,
    /// Whether this is a composite video plane (always 0).
    is_vu: u8,
    /// Color encoding (only relevant for composite video planes).
    color_encoding: u8,
    /// DMA addresses of the planes counted in `num_planes`.
    planes: [u32; 4],
    /// Rotation and / or flipping constant.
    transform: u32,
}

/// Sets up the HDMI controller to output video and audio.
#[track_caller]
pub fn init()
{
    let vbuf = alloc::<[u32; VID_BUF_LEN]>();
    unsafe {
        (*vbuf).iter_mut().for_each(|pix| *pix = 0xFF00FF00);
    }
    let plane_in = SetPlanePropertyInput { display_id: DISP_ID,
                                           plane_id: 0,
                                           img_type: IMG_XRGB8888_TYPE,
                                           layer: 0,
                                           width: SCREEN_WIDTH as _,
                                           height: SCREEN_HEIGHT as _,
                                           pitch: PITCH as _,
                                           vpitch: VPITCH as _,
                                           src_x: 0,
                                           src_y: 0,
                                           src_w: (SCREEN_WIDTH << 16) as _,
                                           src_h: (SCREEN_HEIGHT << 16) as _,
                                           dst_x: 0,
                                           dst_y: 0,
                                           dst_w: SCREEN_WIDTH as _,
                                           dst_h: SCREEN_HEIGHT as _,
                                           alpha: 0xFF,
                                           num_planes: 1,
                                           is_vu: 0,
                                           color_encoding: 0,
                                           planes: [vbuf as usize as u32, 0x0, 0x0, 0x0],
                                           transform: 0 };
    mbox! {SET_PLANE_TAG: plane_in => _};
    // Wait for the video core to prepare the HDMI registers.
    for _ in 0 .. 1000000 {
        spin_loop()
    }
    println!("Video initialized");
    let abuf = alloc::<[u32; AU_BUF_LEN]>();
    synthesize(unsafe { &mut *abuf });
    unsafe {
        HD_AU_CTL.write_volatile(0xB12E);
        let ifcfg = IF_CFG.read_volatile();
        IF_CFG.write_volatile(ifcfg & !0x10);
        while IF_STATUS.read_volatile() & 0x10 != 0 {
            spin_loop();
        }
        let offset = 4 * 9;
        IF_START.add(offset).write_volatile(0xA0184);
        IF_START.add(offset + 1).write_volatile(0x170);
        for idx in 2 .. 9 {
            IF_START.add(offset + idx).write_volatile(0);
        }
        IF_CFG.write_volatile(ifcfg | 0x10010);
        AU_CFG.write_volatile(0xC000003);
        AU_PKTCFG.write_volatile(0x21002003);
        AU_CHMAP.write_volatile(0x10);
        HD_AU_FMT.write_volatile(0x20900);
        HD_AU_THR.write_volatile(0x10101C1C);
        HD_AU_SMP.write_volatile((CLOCK_RATE / SAMPLE_RATE * 2) << 8);
        CRP_CFG.write_volatile(0x1000000 | (SAMPLE_RATE * 128 / 1000));
        CTS0.write_volatile(PIXCLOCK_RATE);
        CTS1.write_volatile(PIXCLOCK_RATE);
        println!("Audio initialized");
        setup_sender(&*abuf, HD_AU_DATA, DREQ);
    }
}

/// Synthesizes two audio tones in different frequencies to each of the stereo
/// channels.
fn synthesize(buf: &mut [u32; AU_BUF_LEN])
{
    let mut cs = [0; 24];
    cs[0 .. 5].copy_from_slice(&[0x4, 0x50, 0x0, 0x2, 0xD2]);
    for (sample, output) in buf.iter_mut().enumerate() {
        let period = if sample & 0x1 == 0 { 240 } else { 160 };
        let mut val = if (sample / period) & 0x1 == 1 {
            0x3FFF << 12
        } else {
            0x0C000 << 12
        };
        let subframe = sample % 384;
        if subframe == 0 {
            // Send the B frame marker configured earlier.
            val |= 0x8;
        } else if subframe & 0x1 == 0 {
            // M subframe.
            val |= 0x2;
        } else {
            // W subframe.
            val |= 0x4;
        }
        let byte = subframe >> 4;
        let bit = (subframe >> 1) & 0x7;
        let cs = (cs[byte] >> bit) & 0x1;
        val |= cs << 30;
        if subframe == 17 || subframe == 24 || subframe == 27 {
            // Set the channel bits.
            val |= 1 << 30;
        }
        // Add the parity bit.
        for bit in 4 .. 31 {
            val ^= ((val >> bit) & 0x1) << 31;
        }
        *output = val;
    }
    fence(Ordering::Release);
}
