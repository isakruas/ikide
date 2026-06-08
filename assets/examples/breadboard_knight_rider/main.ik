# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: Knight Rider.
# Walks a single lit LED across PORTD (PD0..PD7). Add an 8-LED bar wired to
# PD0..PD7 on the Breadboard and press Run.

target atmega328p

import std/gpio
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    0xFF -> %DDRD            # all of PORTD as outputs
    ram mut $mask: u8 = 1
    loop * {
        $mask -> %PORTD      # light the current LED
        @delay_ms(80)
        $mask * 2 -> $mask   # move to the next LED
        ? $mask == 0 {       # past PD7 (0x80 * 2 wraps to 0): restart
            1 -> $mask
        }
    }
}
