# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: Text on ST7789 with std/font.
# Renders "IKIDE" on the 240x240 TFT using the 5x8 glyphs from std/font,
# scaled 4x. Each character is drawn into its own 24x32 window: for every
# glyph row, @font_get_col() supplies the column bytes (bit 0 = top row).
# Board tab: add the ST7789 TFT 240x240 device (DC=PB1, CS=PB2).

target atmega328p

import std/gpio
import std/spi
import std/font
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

const BG_HI: u8 = 0x10      # background RGB565 0x1082 (dark blue)
const BG_LO: u8 = 0x82
const FG_HI: u8 = 0xFF      # text RGB565 0xFFFF (white)
const FG_LO: u8 = 0xFF

@send($v: u8) {
    ram mut $r: u8 = 0
    @spi_transfer($v) -> $r
}

@cmd($c: u8) {
    @digital_write_b(1, 0)   # DC low = command
    @send($c)
}

@dat($d: u8) {
    @digital_write_b(1, 1)   # DC high = data
    @send($d)
}

# Draw window [x0..x1] x [y0..y1], then start the pixel write.
@window($x0: u8, $x1: u8, $y0: u8, $y1: u8) {
    @cmd(0x2A)               # CASET
    @dat(0)
    @dat($x0)
    @dat(0)
    @dat($x1)
    @cmd(0x2B)               # RASET
    @dat(0)
    @dat($y0)
    @dat(0)
    @dat($y1)
    @cmd(0x2C)               # RAMWR
}

# Render one character at ($x, $y), 4x scale: 6 columns (5 glyph + 1 space)
# by 8 rows become a 24x32 pixel cell, streamed row by row.
@draw_char($c: u8, $x: u8, $y: u8) {
    @window($x, $x + 23, $y, $y + 31)
    loop 0..8 -> $row {
        # Bit mask for this glyph row (bit 0 is the top).
        ram mut $mask: u8 = 1
        loop 0..$row -> $i {
            $mask * 2 -> $mask
        }
        loop 0..4 -> $sy {           # vertical scale
            loop 0..6 -> $col {      # 5 glyph columns + 1 spacing column
                ram imut $bits: u8 = @font_get_col($c, $col)
                loop 0..4 -> $sx {   # horizontal scale
                    ? ($bits & $mask) != 0 {
                        @dat(FG_HI)
                        @dat(FG_LO)
                    } : {
                        @dat(BG_HI)
                        @dat(BG_LO)
                    }
                }
            }
        }
    }
}

@main {
    @pin_mode_b(1, 1)        # DC
    @pin_mode_b(2, 1)        # CS
    @spi_init_master_raw()
    @digital_write_b(2, 0)   # select the display

    # Clear the whole panel to the background color.
    @window(0, 239, 0, 239)
    loop 0..57600 -> $i {
        @dat(BG_HI)
        @dat(BG_LO)
    }

    # "IKIDE", centered: 5 cells of 24 px from x=60, baseline at y=104.
    @draw_char(73, 60, 104)  # I
    @draw_char(75, 84, 104)  # K
    @draw_char(73, 108, 104) # I
    @draw_char(68, 132, 104) # D
    @draw_char(69, 156, 104) # E

    loop * {
        @delay_ms(1000)
    }
}
