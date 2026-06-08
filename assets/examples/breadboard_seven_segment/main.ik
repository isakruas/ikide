# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: 7-Segment Counter.
# Counts 0-9 on a common-cathode 7-segment digit. Segments a..g map to
# PD0..PD6 (bit set = segment lit). Wire the 7-segment component's a..g
# terminals to PD0..PD6 on the Breadboard.

target atmega328p

import std/gpio
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

# Segment bitmask (a=bit0 .. g=bit6) for each decimal digit.
@seg_for($d: u8) -> u8 {
    ? $d == 0 { return 0x3F }
    ? $d == 1 { return 0x06 }
    ? $d == 2 { return 0x5B }
    ? $d == 3 { return 0x4F }
    ? $d == 4 { return 0x66 }
    ? $d == 5 { return 0x6D }
    ? $d == 6 { return 0x7D }
    ? $d == 7 { return 0x07 }
    ? $d == 8 { return 0x7F }
    ? $d == 9 { return 0x6F }
    return 0
}

@main {
    0x7F -> %DDRD            # PD0..PD6 as outputs (segments a..g)
    ram mut $d: u8 = 0
    loop * {
        ram mut $pat: u8 = 0
        @seg_for($d) -> $pat
        $pat -> %PORTD
        @delay_ms(700)
        $d + 1 -> $d
        ? $d == 10 {
            0 -> $d
        }
    }
}
