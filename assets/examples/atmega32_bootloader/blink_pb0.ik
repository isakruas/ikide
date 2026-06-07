# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Validation application for the ik serial bootloader (see bootloader.ik).
# Blinks the LED on PB0. It lives in the application section (from 0x0000), so
# it is the image you upload *through the bootloader* — not directly via ISP.

target atmega32

import std/gpio
import std/delay

# The board's CPU clock, in MHz. Set this to match your crystal/oscillator so
# the delays are accurate (ATmega32 ships at 1 MHz on the internal RC by default).
@cpu_mhz() -> u16 {
    return 8
}

@main {
    @pin_mode_b(0, 1)          # PB0 as a digital output
    loop * {
        @toggle_b(0)           # flip PB0
        @delay_ms(500)         # ~0.5 s on, 0.5 s off
    }
}
