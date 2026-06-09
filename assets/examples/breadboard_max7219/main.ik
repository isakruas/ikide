# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: MAX7219 LED Matrix.
# Sends 16-bit (register, data) frames, latched by a LOAD (PB2) rising edge:
# wake from shutdown, then write the eight row registers to draw an X.
# Board tab: add the MAX7219 device; the 8x8 panel renders at the top.

target atmega328p

import std/gpio
import std/spi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@frame($reg: u8, $data: u8) {
    @digital_write_b(2, 0)   # LOAD low while shifting
    ram mut $r: u8 = 0
    @spi_transfer($reg) -> $r
    @spi_transfer($data) -> $r
    @digital_write_b(2, 1)   # LOAD rising edge latches the frame
}

@main {
    @spi_init_master_raw()
    @pin_mode_b(2, 1)

    @frame(0x0C, 0x01)       # shutdown register: normal operation
    @frame(0x01, 0x81)       # rows of an X pattern
    @frame(0x02, 0x42)
    @frame(0x03, 0x24)
    @frame(0x04, 0x18)
    @frame(0x05, 0x18)
    @frame(0x06, 0x24)
    @frame(0x07, 0x42)
    @frame(0x08, 0x81)

    loop * {
        @delay_ms(1000)
    }
}
