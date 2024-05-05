# Bare Metal Raspberry Pi HDMI Audio

This project contains a working implementation of a bare metal HDMI audio driver for the Raspberry Pi 4. Also included are MiniUART, Mailbox, and DMA drivers which I needed in order to debug and boot the Raspberry Pi to a point where HDMI audio can be configured, but documenting those is outside the scope of this project. In the future I might include code for the Raspberry Pi 5 as well, if and when I manage to figure how to do it myself.

Running this code should result in the Raspberry Pi displaying a 1920x1080 green screen over HDMI 0 and playing a square wave audio tone at 200Hz on one channel and another at 300Hz on the other channel. It works at least with the displays on which I tested it, however since I'm not sure I'm respecting the HDMI specification, I cannot guarantee that it works with every display.

## Compilation

This code is guaranteed to compile with the latest nightly version of the Rust programming language at the time of this commit. If you don't have Rust installed, [installation instructions](https://www.rust-lang.org/learn/get-started) can be found on its official website. However since this is a bare metal project, a few extra steps are required to finish setting up Rust.

### Install Nightly Rust

By default Rust installs the stable version, which is appropriate in most situations but is missing some useful features that I use in bare metal code.

To install the nightly version of Rust along with the required bare metal target for cross compilation, type the following in a terminal window:

    rustup toolchain install nightly -t aarch64-unknown-none -c rust-src,clippy

Once that's done, you're ready to cross-compile.

### Compiling the code

Included in this project is a shell script named `build` which can be used to compile it on a Unix-like system.

To compile the code using the provided script, type the following in a terminal window after entering this project's directory:

    ./build

Hopefully the compilation will succeed and a `kernel8.img` binary will be generated in the `boot` directory of this project.

## Running

The easiest way to run bare metal code on the Raspberry Pi is through PXE, which requires properly configured DHCP and TFTP servers. One service that can be used for the task, which is actually what I use on MacOS, is `dnsmasq`, and [configuration instructions](https://www.raspberrypi.com/documentation/computers/remote-access.html#network-boot-your-raspberry-pi) for it can be found on the official Raspberry Pi website.

Below is my `dnsmasq.conf` for reference:

    port=0
    interface=en5
    dhcp-range=192.168.0.2,192.168.0.15,1m
    pxe-service=0,"Raspberry Pi Boot"
    enable-tftp
    tftp-root=/Users/jps/rpi-hdmi/boot

## Development

My main source of information for this project is the Video Core Kernel Mode Setting driver from the [official Raspberry Pi Linux kernel fork](https://github.com/raspberrypi/linux), which is very poorly explained.

Since we're talking about roughly 8000 lines of code, and since I wasn't feeling like implementing it all, I'm relying on the firmware to do most of the heavy lifting by configuring the video part through the Mailbox interface, and then driving the audio part myself.

The gist of this driver is in the following section of `src/hdmi.rs`:

```
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
```

And then there's the synthesizer code that generates IEC958 SPDIF frames:

```
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
```

The following is a list of all the Linux kernel source files from which I extracted all the information that I needed to build this driver:

* `arch/arm/boot/dts/bcm2711.dtsi`
* `drivers/gpu/drm/vc4/vc4_regs.h`
* `drivers/gpu/drm/vc4/vc4_hdmi_regs.h`
* `drivers/gpu/drm/vc4/vc4_hdmi.c`
* `drivers/video/hdmi.c`
* `include/sound/asoundef.h`
* `include/linux/hdmi.h`
