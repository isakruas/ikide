# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: Joystick Mouse on ST7789.
# A mouse pointer driven by the joystick, on a 240x240 TFT: U/L/R/D move the
# arrow, SET clicks (painting the target under the tip with the current
# color), RST opens a color menu where SET picks the pointer color.
#
# The scene (background, three target squares, menu) is procedural: every
# pixel's color is computed by @scene_id(), so erasing the cursor is just
# redrawing the 8x8 patch underneath it — no frame buffer needed in 2 KB of
# SRAM. The UI state (menu flag, target and pointer colors) lives at fixed
# RAM addresses so the drawing helpers can read it without long argument
# lists. Board tab: add the ST7789 (DC=PB1, CS=PB2) and the Joystick
# (PC0..PC5).

target atmega328p

import std/gpio
import std/spi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

# Shared UI state, at fixed SRAM addresses (atmega328p SRAM: 0x100..0x8FF).
const MENU_ADDR: u16 = 0x0500   # 1 = color menu open
const S1_ADDR: u16 = 0x0501     # color id of desktop target 1
const S2_ADDR: u16 = 0x0502     # color id of desktop target 2
const S3_ADDR: u16 = 0x0503     # color id of desktop target 3
const CUR_ADDR: u16 = 0x0504    # pointer color id

@send($v: u8) {
    ram mut $r: u8 = 0
    @spi_transfer($v) -> $r
}

@cmd($c: u8) {
    @digital_write_b(1, 0)
    @send($c)
}

@dat($d: u8) {
    @digital_write_b(1, 1)
    @send($d)
}

@window($x0: u8, $x1: u8, $y0: u8, $y1: u8) {
    @cmd(0x2A)
    @dat(0)
    @dat($x0)
    @dat(0)
    @dat($x1)
    @cmd(0x2B)
    @dat(0)
    @dat($y0)
    @dat(0)
    @dat($y1)
    @cmd(0x2C)
}

# Color ids: 0 background, 1 red, 2 green, 3 blue, 4 menu panel, 5 white.
@col_hi($id: u8) -> u8 {
    ? $id == 1 { return 0xF8 }
    ? $id == 2 { return 0x07 }
    ? $id == 3 { return 0x00 }
    ? $id == 4 { return 0x52 }
    ? $id == 5 { return 0xFF }
    return 0x10
}

@col_lo($id: u8) -> u8 {
    ? $id == 1 { return 0x00 }
    ? $id == 2 { return 0xE0 }
    ? $id == 3 { return 0x1F }
    ? $id == 4 { return 0xAA }
    ? $id == 5 { return 0xFF }
    return 0x82
}

# The whole scene, procedurally: which color id lives at pixel ($x, $y)?
# Reads the menu flag and target colors from the shared state.
@scene_id($x: u8, $y: u8) -> u8 {
    ram ptr u8 $menu_p = MENU_ADDR
    ? *$menu_p != 0 {
        ? $x >= 60 {
            ? $x < 180 {
                ? $y >= 60 {
                    ? $y < 180 {
                        ? $x >= 66 {
                            ? $x < 174 {
                                ? $y >= 66 {
                                    ? $y < 96 { return 1 }
                                }
                                ? $y >= 105 {
                                    ? $y < 135 { return 2 }
                                }
                                ? $y >= 144 {
                                    ? $y < 174 { return 3 }
                                }
                            }
                        }
                        return 4
                    }
                }
            }
        }
    }
    ? $y >= 40 {
        ? $y < 80 {
            ? $x >= 24 {
                ? $x < 64 {
                    ram ptr u8 $s1_p = S1_ADDR
                    return *$s1_p
                }
            }
            ? $x >= 100 {
                ? $x < 140 {
                    ram ptr u8 $s2_p = S2_ADDR
                    return *$s2_p
                }
            }
            ? $x >= 176 {
                ? $x < 216 {
                    ram ptr u8 $s3_p = S3_ADDR
                    return *$s3_p
                }
            }
        }
    }
    return 0
}

# Redraw a rectangle of the scene (full draws, menu toggles, cursor erase).
@draw_scene($x0: u8, $y0: u8, $w: u8, $h: u8) {
    loop 0..$h -> $ry {
        ram imut $y: u8 = $y0 + $ry
        @window($x0, $x0 + $w - 1, $y, $y)
        loop 0..$w -> $rx {
            ram imut $id: u8 = @scene_id($x0 + $rx, $y)
            @dat(@col_hi($id))
            @dat(@col_lo($id))
        }
    }
}

# The 8x8 arrow sprite, one row per call (bit 7 = leftmost pixel).
@arrow_row($r: u8) -> u8 {
    ? $r == 0 { return 0x80 }
    ? $r == 1 { return 0xC0 }
    ? $r == 2 { return 0xE0 }
    ? $r == 3 { return 0xF0 }
    ? $r == 4 { return 0xF8 }
    ? $r == 5 { return 0xD8 }
    ? $r == 6 { return 0x8C }
    return 0x0C
}

# Is sprite bit (7 - $rx) of $row set?
@arrow_bit($row: u8, $rx: u8) -> u8 {
    ram imut $n: u8 = 7 - $rx
    ram mut $mask: u8 = 1
    loop 0..$n -> $i {
        $mask * 2 -> $mask
    }
    ? ($row & $mask) != 0 { return 1 }
    return 0
}

# Draw the pointer at ($x, $y): sprite pixels in the pointer color, the rest
# showing the scene through.
@draw_cursor($x: u8, $y: u8) {
    ram ptr u8 $cur_p = CUR_ADDR
    ram imut $cur: u8 = *$cur_p
    loop 0..8 -> $ry {
        ram imut $row: u8 = @arrow_row($ry)
        ram imut $yy: u8 = $y + $ry
        @window($x, $x + 7, $yy, $yy)
        loop 0..8 -> $rx {
            ? @arrow_bit($row, $rx) != 0 {
                @dat(@col_hi($cur))
                @dat(@col_lo($cur))
            } : {
                ram imut $id: u8 = @scene_id($x + $rx, $yy)
                @dat(@col_hi($id))
                @dat(@col_lo($id))
            }
        }
    }
}

@main {
    @pin_mode_b(1, 1)        # DC
    @pin_mode_b(2, 1)        # CS
    @spi_init_master_raw()
    @digital_write_b(2, 0)

    # Joystick lines: inputs with pull-ups, 0 while pressed.
    loop 0..6 -> $p {
        @pin_mode_c($p, 0)
        @digital_write_c($p, 1)
    }

    # Initial UI state.
    ram ptr u8 $menu_p = MENU_ADDR
    ram ptr u8 $s1_p = S1_ADDR
    ram ptr u8 $s2_p = S2_ADDR
    ram ptr u8 $s3_p = S3_ADDR
    ram ptr u8 $cur_p = CUR_ADDR
    0 -> *$menu_p
    1 -> *$s1_p
    2 -> *$s2_p
    3 -> *$s3_p
    5 -> *$cur_p

    ram mut $cx: u8 = 116    # pointer position (tip)
    ram mut $cy: u8 = 116
    ram mut $pset: u8 = 1    # previous levels for press edges
    ram mut $prst: u8 = 1

    @draw_scene(0, 0, 240, 240)
    @draw_cursor($cx, $cy)

    loop * {
        ram imut $u: u8 = @digital_read_c(0)
        ram imut $d: u8 = @digital_read_c(1)
        ram imut $l: u8 = @digital_read_c(2)
        ram imut $r: u8 = @digital_read_c(3)
        ram imut $set: u8 = @digital_read_c(4)
        ram imut $rst: u8 = @digital_read_c(5)

        # Move while held, 4 px per tick, clamped to the panel.
        ram imut $ox: u8 = $cx
        ram imut $oy: u8 = $cy
        ? $u == 0 {
            ? $cy > 3 { $cy - 4 -> $cy }
        }
        ? $d == 0 {
            ? $cy < 229 { $cy + 4 -> $cy }
        }
        ? $l == 0 {
            ? $cx > 3 { $cx - 4 -> $cx }
        }
        ? $r == 0 {
            ? $cx < 229 { $cx + 4 -> $cx }
        }
        ram mut $mv: u8 = 0
        ? $cx != $ox { 1 -> $mv }
        ? $cy != $oy { 1 -> $mv }
        ? $mv != 0 {
            @draw_scene($ox, $oy, 8, 8)
            @draw_cursor($cx, $cy)
        }

        # SET: click. In the menu, pick the pointer color; on the desktop,
        # paint the target under the tip with the current color.
        ? $set == 0 {
            ? $pset != 0 {
                ram imut $hit: u8 = @scene_id($cx, $cy)
                ? *$menu_p != 0 {
                    ? $hit >= 1 {
                        ? $hit <= 3 {
                            $hit -> *$cur_p
                            0 -> *$menu_p
                            @draw_scene(60, 60, 120, 120)
                            @draw_cursor($cx, $cy)
                        }
                    }
                } : {
                    ? $hit >= 1 {
                        ? $hit <= 5 {
                            ? $cx < 64 { *$cur_p -> *$s1_p }
                            ? $cx >= 100 {
                                ? $cx < 140 { *$cur_p -> *$s2_p }
                            }
                            ? $cx >= 176 { *$cur_p -> *$s3_p }
                            @draw_scene(24, 40, 192, 40)
                            @draw_cursor($cx, $cy)
                        }
                    }
                }
            }
        }

        # RST: toggle the color menu.
        ? $rst == 0 {
            ? $prst != 0 {
                ? *$menu_p == 0 { 1 -> *$menu_p } : { 0 -> *$menu_p }
                @draw_scene(60, 60, 120, 120)
                @draw_cursor($cx, $cy)
            }
        }

        $set -> $pset
        $rst -> $prst
        @delay_ms(40)
    }
}
