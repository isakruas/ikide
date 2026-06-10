# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# gfx: ST7789 drawing helpers for the dodge-car game.
# Low-level SPI byte/command/data, address window, solid rectangle fills and
# 2x-scale (12x16) text rendered from std/font glyphs.
#
# This module has no `target` and no imports of its own: main.ik imports
# std/gpio, std/spi and std/font first, then this file, and everything is
# merged into one compilation unit.

# Shared drawing state at fixed SRAM addresses (atmega328p SRAM 0x100..0x8FF),
# so the helpers can read colors without long argument lists.
const GFX_FG_HI: u16 = 0x0560   # text foreground RGB565, high byte
const GFX_FG_LO: u16 = 0x0561   # text foreground RGB565, low byte
const GFX_BG_HI: u16 = 0x0562   # text background RGB565, high byte
const GFX_BG_LO: u16 = 0x0563   # text background RGB565, low byte
const GFX_FILL_HI: u16 = 0x0564 # fill color RGB565, high byte
const GFX_FILL_LO: u16 = 0x0565 # fill color RGB565, low byte

@gfx_send($v: u8) {
    ram mut $r: u8 = 0
    @spi_transfer($v) -> $r
}

@gfx_cmd($c: u8) {
    @digital_write_b(1, 0)
    @gfx_send($c)
}

@gfx_dat($d: u8) {
    @digital_write_b(1, 1)
    @gfx_send($d)
}

# CASET/RASET/RAMWR: next pixel writes land in the given rectangle.
@gfx_window($x0: u8, $x1: u8, $y0: u8, $y1: u8) {
    @gfx_cmd(0x2A)
    @gfx_dat(0)
    @gfx_dat($x0)
    @gfx_dat(0)
    @gfx_dat($x1)
    @gfx_cmd(0x2B)
    @gfx_dat(0)
    @gfx_dat($y0)
    @gfx_dat(0)
    @gfx_dat($y1)
    @gfx_cmd(0x2C)
}

# DC on PB1, CS on PB2 (the breadboard ST7789 wiring), SPI master mode.
@gfx_init() {
    @pin_mode_b(1, 1)
    @pin_mode_b(2, 1)
    @spi_init_master_raw()
    @digital_write_b(2, 0)
}

@gfx_set_fill($hi: u8, $lo: u8) {
    ram ptr u8 $fh = GFX_FILL_HI
    ram ptr u8 $fl = GFX_FILL_LO
    $hi -> *$fh
    $lo -> *$fl
}

# Solid rectangle in the current fill color.
@gfx_rect($x: u8, $y: u8, $w: u8, $h: u8) {
    ? $w == 0 { return }
    ? $h == 0 { return }
    ram ptr u8 $fh = GFX_FILL_HI
    ram ptr u8 $fl = GFX_FILL_LO
    @gfx_window($x, $x + $w - 1, $y, $y + $h - 1)
    loop 0..$h -> $ry {
        loop 0..$w -> $rx {
            @gfx_dat(*$fh)
            @gfx_dat(*$fl)
        }
    }
}

@gfx_set_text($fg_hi: u8, $fg_lo: u8, $bg_hi: u8, $bg_lo: u8) {
    ram ptr u8 $fgh = GFX_FG_HI
    ram ptr u8 $fgl = GFX_FG_LO
    ram ptr u8 $bgh = GFX_BG_HI
    ram ptr u8 $bgl = GFX_BG_LO
    $fg_hi -> *$fgh
    $fg_lo -> *$fgl
    $bg_hi -> *$bgh
    $bg_lo -> *$bgl
}

# One character at ($x, $y), doubled to 12x16 (5x8 glyph + spacing column),
# in the current text colors. Returns the x of the next character cell.
@gfx_char($c: u8, $x: u8, $y: u8) -> u8 {
    ram ptr u8 $fgh = GFX_FG_HI
    ram ptr u8 $fgl = GFX_FG_LO
    ram ptr u8 $bgh = GFX_BG_HI
    ram ptr u8 $bgl = GFX_BG_LO

    @gfx_window($x, $x + 11, $y, $y + 15)
    loop 0..8 -> $row {
        ram mut $mask: u8 = 1
        loop 0..$row -> $i {
            $mask * 2 -> $mask
        }
        loop 0..2 -> $sy {
            loop 0..6 -> $col {
                ram imut $bits: u8 = @font_get_col($c, $col)
                loop 0..2 -> $sx {
                    ? ($bits & $mask) != 0 {
                        @gfx_dat(*$fgh)
                        @gfx_dat(*$fgl)
                    } : {
                        @gfx_dat(*$bgh)
                        @gfx_dat(*$bgl)
                    }
                }
            }
        }
    }
    return $x + 12
}

# NUL-terminated string at ($x, $y); returns the x after the last glyph.
@gfx_text($s: str ram, $x: u8, $y: u8) -> u8 {
    ram mut $cx: u8 = $x
    ram mut $i: u16 = 0
    loop * {
        ram imut $ch: u8 = *($s + $i)
        ? $ch == 0 { return $cx }
        @gfx_char($ch, $cx, $y) -> $cx
        $i + 1 -> $i
    }
    return $cx
}

# $val as three decimal digits (000..999) in the current text colors.
@gfx_digits($val: u16, $x: u8, $y: u8) {
    ram mut $s: u16 = $val
    ram mut $h: u8 = 0
    loop 0..9 -> $i {
        ? $s >= 100 {
            $s - 100 -> $s
            $h + 1 -> $h
        }
    }
    ram mut $t: u8 = 0
    loop 0..9 -> $i {
        ? $s >= 10 {
            $s - 10 -> $s
            $t + 1 -> $t
        }
    }
    ram imut $u: u8 = $s & 0x0F
    ram mut $cx: u8 = $x
    @gfx_char($h + 48, $cx, $y) -> $cx
    @gfx_char($t + 48, $cx, $y) -> $cx
    @gfx_char($u + 48, $cx, $y) -> $cx
}
