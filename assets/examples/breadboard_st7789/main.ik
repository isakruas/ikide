# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: ST7789 Display.
# Sets a 64x64 draw window on an ST7789 SPI TFT and fills it, alternating red
# and blue. On the Breadboard: SPI tab -> attach "ST7789 TFT 240x240" (DC=PB1,
# CS=PB2); the rendered framebuffer shows in the Schematic tab.

target atmega328p

import std/gpio
import std/spi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

# Control pins: DC = PB1, CS = PB2 (SPI uses PB3=MOSI, PB5=SCK).
@send($v: u8) {
    ram mut $r: u8 = 0
    @spi_transfer($v) -> $r
}
@write_cmd($c: u8) {
    @digital_write_b(1, 0)   # DC low = command
    @send($c)
}
@write_dat($d: u8) {
    @digital_write_b(1, 1)   # DC high = data
    @send($d)
}

# Set the column/row window to the full 240x240 panel (0..239) and start a
# memory write. 239 = 0x00EF, sent as high byte then low byte.
@window() {
    @write_cmd(0x2A)         # CASET
    @write_dat(0)
    @write_dat(0)
    @write_dat(0)
    @write_dat(239)
    @write_cmd(0x2B)         # RASET
    @write_dat(0)
    @write_dat(0)
    @write_dat(0)
    @write_dat(239)
    @write_cmd(0x2C)         # RAMWR
}

# Fill the whole 240x240 display with one RGB565 color (hi, lo bytes).
@fill($hi: u8, $lo: u8) {
    @window()
    loop 0..57600 -> $i {    # 240 * 240 pixels
        @write_dat($hi)
        @write_dat($lo)
    }
}

@main {
    @pin_mode_b(1, 1)        # DC output
    @pin_mode_b(2, 1)        # CS output
    @spi_init_master_raw()
    @digital_write_b(2, 0)   # CS low (selected)

    loop * {
        @fill(0xF8, 0x00)    # red
        @delay_ms(500)
        @fill(0x00, 0x1F)    # blue
        @delay_ms(500)
    }
}
