# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# road: pseudo-3D road scene for the dodge-car game.
# The road converges to a vanishing point at (120, 60): every screen row
# below the horizon gets a half-width from @road_half(), giving white edge
# lines and dashed lane dividers that narrow with distance. The scene is
# procedural — @road_id() answers "what color lives at (x, y)?" — so erasing
# a car is just redrawing the patch underneath it, no frame buffer needed.
#
# Depth is quantized into 8 steps (@step_y); cars are drawn as scaled
# cabin+body+wheels rectangles, so they grow as they approach the player.
#
# This module has no `target` and no imports of its own: main.ik imports
# gfx first, then this file, and everything is merged into one unit.

const HORIZON: u8 = 60

# Road half-width at screen row $y (>= 60): 14 px at the horizon growing
# to ~108 px at the bottom edge.
@road_half($y: u8) -> u8 {
    ram mut $d: u16 = $y
    $d - 60 -> $d
    $d * 9 -> $d
    $d / 17 -> $d
    ram imut $w: u8 = $d + 14
    return $w
}

# Color id at pixel ($x, $y): 0 sky, 1 grass, 2 asphalt, 3 white edge line,
# 4 yellow lane dash, 5 sun.
@road_id($x: u8, $y: u8) -> u8 {
    ? $y < HORIZON {
        ? $y >= 12 {
            ? $y < 36 {
                ? $x >= 184 {
                    ? $x < 208 { return 5 }
                }
            }
        }
        return 0
    }
    ram imut $w: u8 = @road_half($y)
    ram imut $xl: u8 = 120 - $w
    ram imut $xr: u8 = 120 + $w
    ? $x < $xl { return 1 }
    ? $x >= $xr { return 1 }
    ? $x < $xl + 2 { return 3 }
    ? $x >= $xr - 2 { return 3 }
    ram imut $t: u8 = $w / 3
    ram imut $d1: u8 = 120 - $t
    ram imut $d2: u8 = 120 + $t
    ? (($y / 8) & 1) == 0 {
        ? $x >= $d1 {
            ? $x < $d1 + 2 { return 4 }
        }
        ? $x >= $d2 {
            ? $x < $d2 + 2 { return 4 }
        }
    }
    return 2
}

@road_hi($id: u8) -> u8 {
    ? $id == 0 { return 0x86 }   # sky 0x861F
    ? $id == 1 { return 0x05 }   # grass 0x0540
    ? $id == 3 { return 0xFF }   # edge line 0xFFFF
    ? $id == 4 { return 0xFF }   # lane dash 0xFFE0
    ? $id == 5 { return 0xFD }   # sun 0xFD20
    return 0x63                  # asphalt 0x630C
}

@road_lo($id: u8) -> u8 {
    ? $id == 0 { return 0x1F }
    ? $id == 1 { return 0x40 }
    ? $id == 3 { return 0xFF }
    ? $id == 4 { return 0xE0 }
    ? $id == 5 { return 0x20 }
    return 0x0C
}

# $n pixels of color $id on row $y starting at $x0 (one window, fast path).
@road_seg($x0: u8, $n: u8, $id: u8, $y: u8) {
    ? $n == 0 { return }
    @gfx_window($x0, $x0 + $n - 1, $y, $y)
    ram imut $hi: u8 = @road_hi($id)
    ram imut $lo: u8 = @road_lo($id)
    loop 0..$n -> $i {
        @gfx_dat($hi)
        @gfx_dat($lo)
    }
}

# Redraw a rectangle of the scene pixel by pixel (car erase, small patches).
@road_patch($x0: u8, $y0: u8, $w: u8, $h: u8) {
    loop 0..$h -> $ry {
        ram imut $y: u8 = $y0 + $ry
        @gfx_window($x0, $x0 + $w - 1, $y, $y)
        loop 0..$w -> $rx {
            ram imut $id: u8 = @road_id($x0 + $rx, $y)
            @gfx_dat(@road_hi($id))
            @gfx_dat(@road_lo($id))
        }
    }
}

# Full background, row by row with segment fills. The segments follow the
# exact same geometry as @road_id, so patch redraws blend in seamlessly.
@road_full() {
    loop 0..240 -> $y {
        ? $y < 60 {
            ram mut $sun: u8 = 0
            ? $y >= 12 {
                ? $y < 36 { 1 -> $sun }
            }
            ? $sun != 0 {
                @road_seg(0, 184, 0, $y)
                @road_seg(184, 24, 5, $y)
                @road_seg(208, 32, 0, $y)
            } : {
                @road_seg(0, 240, 0, $y)
            }
        } : {
            ram imut $w: u8 = @road_half($y)
            ram imut $xl: u8 = 120 - $w
            ram imut $xr: u8 = 120 + $w
            ram imut $t: u8 = $w / 3
            ram imut $d1: u8 = 120 - $t
            ram imut $d2: u8 = 120 + $t
            ram mut $lane: u8 = 2
            ? (($y / 8) & 1) == 0 { 4 -> $lane }
            @road_seg(0, $xl, 1, $y)
            @road_seg($xl, 2, 3, $y)
            @road_seg($xl + 2, $d1 - $xl - 2, 2, $y)
            @road_seg($d1, 2, $lane, $y)
            @road_seg($d1 + 2, $d2 - $d1 - 2, 2, $y)
            @road_seg($d2, 2, $lane, $y)
            @road_seg($d2 + 2, $xr - $d2 - 4, 2, $y)
            @road_seg($xr - 2, 2, 3, $y)
            @road_seg($xr, 240 - $xr, 1, $y)
        }
    }
}

# ---- cars ------------------------------------------------------------

# Bottom y of an enemy at depth step $t (0 = far, 7 = right above the player).
@step_y($t: u8) -> u8 {
    ? $t == 0 { return 70 }
    ? $t == 1 { return 78 }
    ? $t == 2 { return 88 }
    ? $t == 3 { return 102 }
    ? $t == 4 { return 120 }
    ? $t == 5 { return 144 }
    ? $t == 6 { return 174 }
    return 210
}

# Car size scales with the bottom row: tiny near the horizon, wide up close.
@car_w($by: u8) -> u8 {
    return (($by - 60) / 4) + 8
}

@car_h($by: u8) -> u8 {
    ram imut $w: u8 = @car_w($by)
    return ($w / 2) + 2
}

# X center of lane 0/1/2 at the row $by, following the road's perspective.
@lane_cx($lane: u8, $by: u8) -> u8 {
    ram imut $rw: u8 = @road_half($by)
    ram imut $o: u8 = ($rw * 2) / 3
    ? $lane == 0 { return 120 - $o }
    ? $lane == 2 { return 120 + $o }
    return 120
}

# A car seen from behind: gray cabin on top, colored body, black wheels.
# ($cx, $by) is the bottom-center; the sprite spans $h rows ending at $by.
@draw_car($cx: u8, $by: u8, $w: u8, $h: u8, $bhi: u8, $blo: u8) {
    ram imut $x0: u8 = $cx - ($w / 2)
    ram imut $top: u8 = $by - $h + 1
    ram imut $hc: u8 = ($h / 3) + 1
    @gfx_set_fill(0xC6, 0x18)
    @gfx_rect($cx - ($w / 4), $top, $w / 2, $hc)
    @gfx_set_fill($bhi, $blo)
    @gfx_rect($x0, $top + $hc, $w, $h - $hc)
    ram imut $ww: u8 = ($w / 6) + 2
    ram imut $hw: u8 = ($h / 4) + 1
    @gfx_set_fill(0x00, 0x00)
    @gfx_rect($x0, $by - $hw + 1, $ww, $hw)
    @gfx_rect($x0 + $w - $ww, $by - $hw + 1, $ww, $hw)
}

# Put the road back over a car's bounding box.
@erase_car_box($cx: u8, $by: u8, $w: u8, $h: u8) {
    @road_patch($cx - ($w / 2), $by - $h + 1, $w, $h)
}

@draw_enemy($t: u8, $lane: u8) {
    ram imut $by: u8 = @step_y($t)
    ram imut $w: u8 = @car_w($by)
    ram imut $h: u8 = @car_h($by)
    ram imut $cx: u8 = @lane_cx($lane, $by)
    @draw_car($cx, $by, $w, $h, 0xF8, 0x00)
}

@erase_enemy($t: u8, $lane: u8) {
    ram imut $by: u8 = @step_y($t)
    ram imut $w: u8 = @car_w($by)
    ram imut $h: u8 = @car_h($by)
    ram imut $cx: u8 = @lane_cx($lane, $by)
    @erase_car_box($cx, $by, $w, $h)
}

@draw_player($lane: u8) {
    ram imut $by: u8 = 234
    ram imut $w: u8 = @car_w($by)
    ram imut $h: u8 = @car_h($by)
    ram imut $cx: u8 = @lane_cx($lane, $by)
    @draw_car($cx, $by, $w, $h, 0x04, 0x9F)
}

@erase_player($lane: u8) {
    ram imut $by: u8 = 234
    ram imut $w: u8 = @car_w($by)
    ram imut $h: u8 = @car_h($by)
    ram imut $cx: u8 = @lane_cx($lane, $by)
    @erase_car_box($cx, $by, $w, $h)
}
