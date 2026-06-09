# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: SSD1306 OLED.
# Initializes the display in horizontal addressing mode and fills the full
# 128x64 panel with a checkerboard pattern.
# Board tab: add the SSD1306 device; the panel renders at the top.

target atmega328p

import std/twi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

# One command transaction: control byte 0x00, then the command bytes.
@cmd1($c: u8) {
    @twi_start()
    @twi_write(0x78)         # 0x3C << 1 | write
    @twi_write(0x00)
    @twi_write($c)
    @twi_stop()
}

@cmd3($c: u8, $a: u8, $b: u8) {
    @twi_start()
    @twi_write(0x78)
    @twi_write(0x00)
    @twi_write($c)
    @twi_write($a)
    @twi_write($b)
    @twi_stop()
}

@main {
    @twi_init(72)

    @cmd1(0xAF)              # display on
    @twi_start()             # memory mode: horizontal (0x20, 0x00)
    @twi_write(0x78)
    @twi_write(0x00)
    @twi_write(0x20)
    @twi_write(0x00)
    @twi_stop()
    @cmd3(0x21, 0, 127)      # column window 0..127
    @cmd3(0x22, 0, 7)        # page window 0..7

    loop * {
        # One data transaction streams the whole frame (128 * 8 bytes).
        @twi_start()
        @twi_write(0x78)
        @twi_write(0x40)
        loop 0..1024 -> $i {
            ? ($i & 1) != 0 {
                @twi_write(0x55)
            } : {
                @twi_write(0xAA)
            }
        }
        @twi_stop()
        @delay_ms(1000)
    }
}
