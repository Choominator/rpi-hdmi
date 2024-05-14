# Bare Metal Raspberry Pi HDMI Audio

This project contains a working implementation of a bare metal HDMI audio driver for the Raspberry Pi 4. An implementation for the Raspberry pi 5 can be found on the `rpi5` branch.

Running this code should result in the Raspberry Pi displaying a 1920x1080 green screen over HDMI 0 and playing a square wave audio tone at 200Hz on one channel and another at 300Hz on the other channel. It works at least with the displays on which I tested it, however since I'm not sure I'm respecting the HDMI specification, I cannot guarantee that it works with every display.

## Compilation

This code is guaranteed to compile with the latest nightly version of the Rust programming language at the time of this commit. If you don't have Rust installed, [installation instructions](https://www.rust-lang.org/learn/get-started) can be found on its official website. However since this is a bare metal project, a few extra steps are required to finish setting up Rust.

### Install Nightly Rust

By default Rust installs the stable version for your system, which is appropriate in most situations but is missing some useful features, and a bare metal target for AArch64 also needs to be installed to enable cross compilation for the Raspberry Pi.

To install the nightly version of Rust along with the required bare metal target, type the following in a terminal window:

    rustup toolchain install nightly -t aarch64-unknown-none -c rust-src,clippy

Once that's done, you're ready to build this project.

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

The following is a list of all the Linux kernel source files from which I extracted all the information that I needed to build this driver:

* `arch/arm/boot/dts/broadcom/bcm2711.dtsi`
* `drivers/gpu/drm/vc4/vc4_regs.h`
* `drivers/gpu/drm/vc4/vc4_hdmi_regs.h`
* `drivers/gpu/drm/vc4/vc4_hdmi.c`
* `drivers/video/hdmi.c`
* `include/sound/asoundef.h`
* `include/linux/hdmi.h`

Since we're talking about roughly 8000 lines of code, and since I wasn't feeling like implementing it all, I'm relying on the firmware to do most of the heavy lifting by configuring the video part through the Mailbox interface, and then driving the audio part myself. The gist of this driver is in `src/hdmi.rs`, everything else is just boilerplate code to configure the single-threaded bare metal environment that I needed to implement and debug the driver.
