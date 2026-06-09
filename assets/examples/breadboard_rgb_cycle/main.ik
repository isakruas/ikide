# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: RGB LED Cycle.
# Steps an RGB LED through the eight on/off color combinations.
# Board tab: add an RGB LED wired r=PB1, g=PB2, b=PB3.

target atmega328p

import std/gpio
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @pin_mode_b(1, 1)
    @pin_mode_b(2, 1)
    @pin_mode_b(3, 1)
    loop * {
        loop 0..8 -> $i {
            ? ($i & 1) != 0 { @digital_write_b(1, 1) } : { @digital_write_b(1, 0) }
            ? ($i & 2) != 0 { @digital_write_b(2, 1) } : { @digital_write_b(2, 0) }
            ? ($i & 4) != 0 { @digital_write_b(3, 1) } : { @digital_write_b(3, 0) }
            @delay_ms(400)
        }
    }
}
