# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: PCF8574 Port Expander.
# Walks a single set bit across the expander's eight outputs over I2C.
# Board tab: add the PCF8574 device; its LED bar shows the latched port.

target atmega328p

import std/twi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @twi_init(72)
    ram mut $pat: u8 = 1
    loop * {
        @twi_start()
        @twi_write(0x40)     # 0x20 << 1 | write
        @twi_write($pat)
        @twi_stop()
        $pat * 2 -> $pat
        ? $pat == 0 {
            1 -> $pat
        }
        @delay_ms(150)
    }
}
