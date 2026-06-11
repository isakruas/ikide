# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: Dodge Car 3D on ST7789.
# A pseudo-3D dodging game on the 240x240 TFT: the road converges to a
# vanishing point on the horizon and enemy cars grow as they rush toward
# you through 8 depth steps. L/R switch between the three lanes, dodge to
# score, crash and it's game over.
#
# A config menu picks the difficulty with U/D + SET: EASY (slow, 1 car),
# NORMAL (faster, 2 cars), HARD (faster still, 2 cars). RST during the race
# returns to the menu.
#
# If the AT24C256 I2C EEPROM is present (PC4=SDA, PC5=SCL), a "RANKING" option
# is added to the menu and high scores can be saved using an on-screen keyboard.
#
# The scene is procedural (road.ik) and the drawing helpers live in gfx.ik,
# so this file is only the game state machine. Board tab: add the ST7789
# (DC=PB1, CS=PB2), the AT24C256 EEPROM, and the Joystick (U=PC0, D=PC1, L=PC2,
# R=PC3, SET=PD4, RST=PD5 to avoid TWI/I2C pin conflicts on PC4/PC5).

target atmega328p

import std/gpio
import std/spi
import std/delay
import std/font
import std/twi
import gfx
import road

@cpu_mhz() -> u16 {
    return 16
}

# Game state at fixed SRAM addresses (gfx uses 0x0560..0x0565).
const SEED_ADDR: u16 = 0x0566   # PRNG seed
const E_BASE: u16 = 0x0570      # 2 enemies x 4 bytes: t, lane, cnt, active

@rnd() -> u8 {
    ram ptr u8 $sp = SEED_ADDR
    ram mut $s: u8 = *$sp
    $s * 13 -> $s
    $s + 7 -> $s
    $s -> *$sp
    return $s
}

# Score digits, black on sky, in the top-left corner.
@draw_hud($v: u16) {
    @gfx_set_text(0x00, 0x00, 0x86, 0x1F)
    @gfx_digits($v, 4, 4)
}

# '>' marker next to the selected difficulty row.
@draw_marker($d: u8, $eeprom_ok: u8) {
    ram mut $t: u8 = 0
    @gfx_set_text(0xFF, 0xE0, 0x00, 0x00)
    ? $eeprom_ok != 0 {
        @gfx_char(32, 60, 77) -> $t
        @gfx_char(32, 60, 107) -> $t
        @gfx_char(32, 60, 137) -> $t
        @gfx_char(32, 60, 167) -> $t
        ram mut $y: u8 = 77
        ? $d == 1 { 107 -> $y }
        ? $d == 2 { 137 -> $y }
        ? $d == 3 { 167 -> $y }
        @gfx_char(62, 60, $y) -> $t
    } : {
        @gfx_char(32, 60, 92) -> $t
        @gfx_char(32, 60, 122) -> $t
        @gfx_char(32, 60, 152) -> $t
        ram mut $y: u8 = 92
        ? $d == 1 { 122 -> $y }
        ? $d == 2 { 152 -> $y }
        @gfx_char(62, 60, $y) -> $t
    }
}

# Advance enemy $idx by one tick. Returns 0 = nothing, 1 = the player dodged
# it (score!), 2 = crash.
@enemy_tick($idx: u8, $adv: u8, $plane: u8) -> u8 {
    ram ptr u8 $e = E_BASE
    ram mut $o0: u16 = 0
    ? $idx != 0 { 4 -> $o0 }
    ram imut $o1: u16 = $o0 + 1
    ram imut $o2: u16 = $o0 + 2
    ram imut $o3: u16 = $o0 + 3

    ? *($e + $o3) == 0 { return 0 }
    ram imut $t: u8 = *($e + $o0)
    ram imut $lane: u8 = *($e + $o1)
    ram mut $cnt: u8 = *($e + $o2)
    $cnt + 1 -> $cnt
    ? $cnt < $adv {
        $cnt -> *($e + $o2)
        return 0
    }
    0 -> *($e + $o2)

    @erase_enemy($t, $lane)
    ? $t == 7 {
        # Reaching the player's row: same lane is a crash, otherwise the car
        # is dodged and respawns far away in a random lane.
        ? $lane == $plane { return 2 }
        0 -> *($e + $o0)
        ram imut $nl: u8 = @rnd() % 3
        $nl -> *($e + $o1)
        @draw_enemy(0, $nl)
        return 1
    }
    ram imut $nt: u8 = $t + 1
    $nt -> *($e + $o0)
    @draw_enemy($nt, $lane)
    ? $nt == 7 {
        ? $lane == $plane { return 2 }
    }
    return 0
}

# --- Leaderboard / EEPROM support ---

@eeprom_detect() -> u8 {
    # Write a test byte 0xC3 at address 0x7FFF (last byte of AT24C256)
    @twi_start()
    @twi_write(0xA0)
    @twi_write(0x7F)
    @twi_write(0xFF)
    @twi_write(0xC3)
    @twi_stop()
    @delay_ms(10)

    # Read it back
    @twi_start()
    @twi_write(0xA0)
    @twi_write(0x7F)
    @twi_write(0xFF)
    @twi_start()
    @twi_write(0xA1)
    ram mut $v: u8 = 0
    @twi_read_nack() -> $v
    @twi_stop()

    ? $v == 0xC3 { return 1 }
    return 0
}

@eeprom_write_leaderboard($buf_ptr: ptr ram u8) {
    @twi_start()
    @twi_write(0xA0)
    @twi_write(0x00)
    @twi_write(0x00)
    loop 0..17 -> $i {
        @twi_write(*($buf_ptr + $i))
    }
    @twi_stop()
    @delay_ms(10)
}

@eeprom_read_leaderboard($buf_ptr: ptr ram u8) {
    @twi_start()
    @twi_write(0xA0)
    @twi_write(0x00)
    @twi_write(0x00)
    @twi_start()
    @twi_write(0xA1)
    loop 0..16 -> $i {
        ram mut $temp: u8 = 0
        @twi_read_ack() -> $temp
        $temp -> *($buf_ptr + $i)
    }
    ram mut $temp2: u8 = 0
    @twi_read_nack() -> $temp2
    $temp2 -> *($buf_ptr + 16)
    @twi_stop()
}

@leaderboard_init_defaults($buf_ptr: ptr ram u8) {
    0xD0 -> *($buf_ptr + 0)
    0x6E -> *($buf_ptr + 1)

    # Slot 0
    0 -> *($buf_ptr + 2)
    0 -> *($buf_ptr + 3)
    65 -> *($buf_ptr + 4)
    65 -> *($buf_ptr + 5)
    65 -> *($buf_ptr + 6)

    # Slot 1
    0 -> *($buf_ptr + 7)
    0 -> *($buf_ptr + 8)
    66 -> *($buf_ptr + 9)
    66 -> *($buf_ptr + 10)
    66 -> *($buf_ptr + 11)

    # Slot 2
    0 -> *($buf_ptr + 12)
    0 -> *($buf_ptr + 13)
    67 -> *($buf_ptr + 14)
    67 -> *($buf_ptr + 15)
    67 -> *($buf_ptr + 16)

    @eeprom_write_leaderboard($buf_ptr)
}

@get_score($buf_ptr: ptr ram u8, $idx: u8) -> u16 {
    ram mut $offset: u16 = 2
    ? $idx == 1 { 7 -> $offset }
    ? $idx == 2 { 12 -> $offset }
    ram imut $hi: u8 = *($buf_ptr + $offset)
    ram imut $lo: u8 = *($buf_ptr + $offset + 1)
    ram mut $val: u16 = $hi
    $val * 256 -> $val
    ram mut $lo_u16: u16 = $lo
    $val + $lo_u16 -> $val
    return $val
}

@set_score($buf_ptr: ptr ram u8, $idx: u8, $val: u16) {
    ram mut $offset: u16 = 2
    ? $idx == 1 { 7 -> $offset }
    ? $idx == 2 { 12 -> $offset }
    ram imut $hi: u8 = $val / 256
    ram imut $lo: u8 = $val % 256
    $hi -> *($buf_ptr + $offset)
    $lo -> *($buf_ptr + $offset + 1)
}

@set_name($buf_ptr: ptr ram u8, $idx: u8, $c0: u8, $c1: u8, $c2: u8) {
    ram mut $offset: u16 = 2
    ? $idx == 1 { 7 -> $offset }
    ? $idx == 2 { 12 -> $offset }
    $c0 -> *($buf_ptr + $offset + 2)
    $c1 -> *($buf_ptr + $offset + 3)
    $c2 -> *($buf_ptr + $offset + 4)
}

@is_high_score($buf_ptr: ptr ram u8, $new_score: u16) -> u8 {
    ram imut $s2: u16 = @get_score($buf_ptr, 2)
    ? $new_score > $s2 { return 1 }
    return 0
}

@insert_high_score($buf_ptr: ptr ram u8, $new_score: u16, $c0: u8, $c1: u8, $c2: u8) {
    ram imut $s0: u16 = @get_score($buf_ptr, 0)
    ram imut $s1: u16 = @get_score($buf_ptr, 1)

    ? $new_score > $s0 {
        # Shift Slot 1 -> Slot 2
        ram imut $s1_val: u16 = @get_score($buf_ptr, 1)
        ram imut $n1_0: u8 = *($buf_ptr + 9)
        ram imut $n1_1: u8 = *($buf_ptr + 10)
        ram imut $n1_2: u8 = *($buf_ptr + 11)
        @set_score($buf_ptr, 2, $s1_val)
        @set_name($buf_ptr, 2, $n1_0, $n1_1, $n1_2)

        # Shift Slot 0 -> Slot 1
        ram imut $s0_val: u16 = @get_score($buf_ptr, 0)
        ram imut $n0_0: u8 = *($buf_ptr + 4)
        ram imut $n0_1: u8 = *($buf_ptr + 5)
        ram imut $n0_2: u8 = *($buf_ptr + 6)
        @set_score($buf_ptr, 1, $s0_val)
        @set_name($buf_ptr, 1, $n0_0, $n0_1, $n0_2)

        # Insert new at Slot 0
        @set_score($buf_ptr, 0, $new_score)
        @set_name($buf_ptr, 0, $c0, $c1, $c2)
    } : {
        ? $new_score > $s1 {
            # Shift Slot 1 -> Slot 2
            ram imut $s1_val: u16 = @get_score($buf_ptr, 1)
            ram imut $n1_0: u8 = *($buf_ptr + 9)
            ram imut $n1_1: u8 = *($buf_ptr + 10)
            ram imut $n1_2: u8 = *($buf_ptr + 11)
            @set_score($buf_ptr, 2, $s1_val)
            @set_name($buf_ptr, 2, $n1_0, $n1_1, $n1_2)

            # Insert new at Slot 1
            @set_score($buf_ptr, 1, $new_score)
            @set_name($buf_ptr, 1, $c0, $c1, $c2)
        } : {
            # Insert new at Slot 2
            @set_score($buf_ptr, 2, $new_score)
            @set_name($buf_ptr, 2, $c0, $c1, $c2)
        }
    }

    @eeprom_write_leaderboard($buf_ptr)
}

@get_key_char($row: u8, $col: u8) -> u8 {
    ? $row == 0 {
        ? $col == 0 { return 81 } # Q
        ? $col == 1 { return 87 } # W
        ? $col == 2 { return 69 } # E
        ? $col == 3 { return 82 } # R
        ? $col == 4 { return 84 } # T
        ? $col == 5 { return 89 } # Y
        ? $col == 6 { return 85 } # U
        ? $col == 7 { return 73 } # I
        ? $col == 8 { return 79 } # O
        ? $col == 9 { return 80 } # P
    }
    ? $row == 1 {
        ? $col == 0 { return 65 } # A
        ? $col == 1 { return 83 } # S
        ? $col == 2 { return 68 } # D
        ? $col == 3 { return 70 } # F
        ? $col == 4 { return 71 } # G
        ? $col == 5 { return 72 } # H
        ? $col == 6 { return 74 } # J
        ? $col == 7 { return 75 } # K
        ? $col == 8 { return 76 } # L
        ? $col == 9 { return 8 }  # Backspace (ASCII 8)
    }
    ? $row == 2 {
        ? $col == 0 { return 90 } # Z
        ? $col == 1 { return 88 } # X
        ? $col == 2 { return 67 } # C
        ? $col == 3 { return 86 } # V
        ? $col == 4 { return 66 } # B
        ? $col == 5 { return 78 } # N
        ? $col == 6 { return 77 } # M
        ? $col == 7 { return 32 } # Space
        ? $col == 8 { return 32 } # Space
        ? $col == 9 { return 10 } # Enter/OK (ASCII 10)
    }
    return 0
}

@draw_key($row: u8, $col: u8, $selected: u8) {
    ram imut $x: u8 = 20 + $col * 20
    ram imut $y: u8 = 120 + $row * 25

    ? $selected != 0 {
        @gfx_set_text(0x00, 0x00, 0xFF, 0xFF) # Black on White
    } : {
        @gfx_set_text(0xFF, 0xFF, 0x00, 0x00) # White on Black
    }

    ram imut $ch: u8 = @get_key_char($row, $col)
    ram mut $t: u8 = 0

    ? $ch == 8 {
        @gfx_char(60, $x, $y) -> $t
    } : {
        ? $ch == 10 {
            @gfx_char(79, $x, $y) -> $t
            @gfx_char(75, $x + 10, $y) -> $t
        } : {
            @gfx_char($ch, $x, $y) -> $t
        }
    }
}

@draw_keyboard($sel_row: u8, $sel_col: u8) {
    loop 0..3 -> $r {
        loop 0..10 -> $c {
            ram mut $sel: u8 = 0
            ? $r == $sel_row {
                ? $c == $sel_col {
                    1 -> $sel
                }
            }
            @draw_key($r, $c, $sel)
        }
    }
}

@draw_entered_name($c0: u8, $c1: u8, $c2: u8) {
    @gfx_set_text(0x07, 0xE0, 0x00, 0x00) # Green on Black
    ram mut $t: u8 = 0
    @gfx_char($c0, 120, 75) -> $t
    @gfx_char($c1, 136, 75) -> $t
    @gfx_char($c2, 152, 75) -> $t
}

@draw_rank_slot($buf_ptr: ptr ram u8, $idx: u8, $y: u8) {
    ram mut $offset: u16 = 2
    ? $idx == 1 { 7 -> $offset }
    ? $idx == 2 { 12 -> $offset }

    ram imut $c0: u8 = *($buf_ptr + $offset + 2)
    ram imut $c1: u8 = *($buf_ptr + $offset + 3)
    ram imut $c2: u8 = *($buf_ptr + $offset + 4)

    ram mut $t: u8 = 0
    @gfx_char($c0, 70, $y) -> $t
    @gfx_char($c1, 82, $y) -> $t
    @gfx_char($c2, 94, $y) -> $t

    @gfx_char(45, 120, $y) -> $t

    ram imut $sc: u16 = @get_score($buf_ptr, $idx)
    @gfx_digits($sc, 145, $y)
}

@main {
    @gfx_init()

    # Joystick PC0..PC3 = U, D, L, R: inputs with pull-ups.
    loop 0..4 -> $p {
        @pin_mode_c($p, 0)
        @digital_write_c($p, 1)
    }

    # Joystick SET = PD4, RST = PD5: inputs with pull-ups.
    @pin_mode_d(4, 0)
    @digital_write_d(4, 1)
    @pin_mode_d(5, 0)
    @digital_write_d(5, 1)

    ram ptr u8 $seed = SEED_ADDR
    42 -> *$seed
    ram ptr u8 $eb = E_BASE

    # 0 menu init, 1 menu, 2 game init, 3 game, 4 game-over init, 5 game over
    ram mut $state: u8 = 0
    ram mut $diff: u8 = 1    # 0 easy, 1 normal, 2 hard, 3 ranking
    ram mut $adv: u8 = 3     # ticks per enemy depth step
    ram mut $nen: u8 = 2     # enemies in play
    ram mut $score: u16 = 0
    ram mut $plane: u8 = 1   # player lane
    ram mut $e2wait: u8 = 0  # ticks until enemy 2 joins

    # Previous button levels for press edges.
    ram mut $pu: u8 = 1
    ram mut $pd: u8 = 1
    ram mut $pl: u8 = 1
    ram mut $pr: u8 = 1
    ram mut $ps: u8 = 1

    ram mut $sx: u8 = 0      # text-cursor scratch

    # Leaderboard and EEPROM variables
    ram mut $leaderboard: u8[17] = 0
    ram mut $eeprom_ok: u8 = 0

    # Keyboard selection
    ram mut $sel_row: u8 = 0
    ram mut $sel_col: u8 = 0

    # Name buffer and length
    ram mut $n_len: u8 = 0
    ram mut $n0: u8 = 95
    ram mut $n1: u8 = 95
    ram mut $n2: u8 = 95

    # TWI and EEPROM Initialization
    @twi_init(72)
    @eeprom_detect() -> $eeprom_ok
    ? $eeprom_ok != 0 {
        @eeprom_read_leaderboard(&$leaderboard[0])
        # Verify signature
        ram imut $sig0: u8 = $leaderboard[0]
        ram imut $sig1: u8 = $leaderboard[1]
        ram mut $sig_ok: u8 = 0
        ? $sig0 == 0xD0 {
            ? $sig1 == 0x6E {
                1 -> $sig_ok
            }
        }
        ? $sig_ok == 0 {
            @leaderboard_init_defaults(&$leaderboard[0])
        }
    }

    loop * {
        switch $state {
            0 -> {
                # --- menu: title, difficulty list, marker ---
                @gfx_set_fill(0x00, 0x00)
                @gfx_rect(0, 0, 240, 240)
                @gfx_set_text(0xFF, 0xFF, 0x00, 0x00)
                ram str $title = "DODGE CAR 3D"
                @gfx_text($title, 48, 28) -> $sx
                ram str $m0 = "EASY"
                ram str $m1 = "NORMAL"
                ram str $m2 = "HARD"

                ? $eeprom_ok != 0 {
                    @gfx_text($m0, 84, 77) -> $sx
                    @gfx_text($m1, 84, 107) -> $sx
                    @gfx_text($m2, 84, 137) -> $sx
                    ram str $m3 = "RANKING"
                    @gfx_text($m3, 84, 167) -> $sx
                } : {
                    @gfx_text($m0, 84, 92) -> $sx
                    @gfx_text($m1, 84, 122) -> $sx
                    @gfx_text($m2, 84, 152) -> $sx
                }

                @gfx_set_text(0x07, 0xE0, 0x00, 0x00)
                ram str $hint = "SET = SELECT"
                @gfx_text($hint, 60, 212) -> $sx
                @draw_marker($diff, $eeprom_ok)
                1 -> $state
            }
            1 -> {
                # --- menu loop: U/D select, SET starts ---
                ram imut $u: u8 = @digital_read_c(0)
                ram imut $d: u8 = @digital_read_c(1)
                ram imut $set: u8 = @digital_read_d(4)
                ? $u == 0 {
                    ? $pu != 0 {
                        ? $diff > 0 {
                            $diff - 1 -> $diff
                            @draw_marker($diff, $eeprom_ok)
                        }
                    }
                }
                ? $d == 0 {
                    ? $pd != 0 {
                        ram mut $max_diff: u8 = 2
                        ? $eeprom_ok != 0 { 3 -> $max_diff }
                        ? $diff < $max_diff {
                            $diff + 1 -> $diff
                            @draw_marker($diff, $eeprom_ok)
                        }
                    }
                }
                ? $set == 0 {
                    ? $ps != 0 {
                        ? $diff == 3 {
                            7 -> $state # Go to ranking init
                        } : {
                            2 -> $state # Go to game init
                        }
                    }
                }
                $u -> $pu
                $d -> $pd
                $set -> $ps
                @delay_ms(40)
            }
            2 -> {
                # --- new race: difficulty params, scene, cars ---
                4 -> $adv
                1 -> $nen
                ? $diff == 1 {
                    3 -> $adv
                    2 -> $nen
                }
                ? $diff == 2 {
                    2 -> $adv
                    2 -> $nen
                }
                0 -> $score
                1 -> $plane

                # enemy 1: far away, center lane; enemy 2 joins later
                0 -> *$eb
                1 -> *($eb + 1)
                0 -> *($eb + 2)
                1 -> *($eb + 3)
                0 -> *($eb + 4)
                ram imut $rl: u8 = @rnd() % 3
                $rl -> *($eb + 5)
                0 -> *($eb + 6)
                0 -> *($eb + 7)
                0 -> $e2wait
                ? $nen == 2 { 16 -> $e2wait }

                @road_full()
                @draw_hud($score)
                @draw_player($plane)
                @draw_enemy(0, 1)
                1 -> $pl
                1 -> $pr
                3 -> $state
            }
            3 -> {
                # --- race tick ---
                ram imut $gl: u8 = @digital_read_c(2)
                ram imut $gr: u8 = @digital_read_c(3)
                ram imut $rst: u8 = 1
                # ? $rst == 0 { 0 -> $state }

                ? $state == 3 {
                    # lane change on press edges
                    ram mut $nl: u8 = $plane
                    ? $gl == 0 {
                        ? $pl != 0 {
                            ? $plane > 0 { $plane - 1 -> $nl }
                        }
                    }
                    ? $gr == 0 {
                        ? $pr != 0 {
                            ? $plane < 2 { $plane + 1 -> $nl }
                        }
                    }
                    ? $nl != $plane {
                        @erase_player($plane)
                        $nl -> $plane
                        @draw_player($plane)
                    }
                    $gl -> $pl
                    $gr -> $pr

                    # enemy 2 joins the race a few ticks after the start
                    ? $e2wait > 0 {
                        $e2wait - 1 -> $e2wait
                        ? $e2wait == 0 {
                            1 -> *($eb + 7)
                            ram imut $l2: u8 = *($eb + 5)
                            @draw_enemy(0, $l2)
                        }
                    }

                    ram imut $ev1: u8 = @enemy_tick(0, $adv, $plane)
                    ? $ev1 == 1 {
                        $score + 1 -> $score
                        @draw_hud($score)
                    }
                    ? $ev1 == 2 {
                        ? $eeprom_ok != 0 {
                            ram imut $is_hi: u8 = @is_high_score(&$leaderboard[0], $score)
                            ? $is_hi != 0 {
                                6 -> $state
                            } : {
                                4 -> $state
                            }
                        } : {
                            4 -> $state
                        }
                    }
                    ? $state == 3 {
                        ram imut $ev2: u8 = @enemy_tick(1, $adv, $plane)
                        ? $ev2 == 1 {
                            $score + 1 -> $score
                            @draw_hud($score)
                        }
                        ? $ev2 == 2 {
                            ? $eeprom_ok != 0 {
                                ram imut $is_hi: u8 = @is_high_score(&$leaderboard[0], $score)
                                ? $is_hi != 0 {
                                    6 -> $state
                                } : {
                                    4 -> $state
                                }
                            } : {
                                4 -> $state
                            }
                        }
                    }
                    ? $state == 3 { @delay_ms(35) }
                }
            }
            4 -> {
                # --- crash: panel with the final score ---
                @gfx_set_fill(0xF8, 0x00)
                @gfx_rect(40, 80, 160, 80)
                @gfx_set_text(0xFF, 0xFF, 0xF8, 0x00)
                ram str $go = "GAME OVER"
                @gfx_text($go, 66, 92) -> $sx
                ram str $sc = "SCORE"
                @gfx_text($sc, 66, 116) -> $sx
                @gfx_digits($score, 138, 116)
                ram str $bk = "SET = MENU"
                @gfx_text($bk, 60, 140) -> $sx
                1 -> $ps
                5 -> $state
            }
            5 -> {
                # --- game over loop: SET returns to the menu ---
                ram imut $set2: u8 = @digital_read_d(4)
                ? $set2 == 0 {
                    ? $ps != 0 { 0 -> $state }
                }
                $set2 -> $ps
                @delay_ms(40)
            }
            6 -> {
                # --- Keyboard screen init ---
                @gfx_set_fill(0x00, 0x00)
                @gfx_rect(0, 0, 240, 240)

                @gfx_set_text(0xFF, 0xE0, 0x00, 0x00) # Yellow on Black
                ram str $kb_title = "NEW HIGH SCORE!"
                @gfx_text($kb_title, 36, 15) -> $sx

                @gfx_set_text(0xFF, 0xFF, 0x00, 0x00) # White on Black
                ram str $kb_score = "SCORE:"
                @gfx_text($kb_score, 50, 45) -> $sx
                @gfx_digits($score, 134, 45)

                ram str $kb_name = "NAME:"
                @gfx_text($kb_name, 50, 75) -> $sx

                0 -> $n_len
                95 -> $n0
                95 -> $n1
                95 -> $n2
                @draw_entered_name($n0, $n1, $n2)

                0 -> $sel_row
                0 -> $sel_col
                @draw_keyboard($sel_row, $sel_col)

                1 -> $pu
                1 -> $pd
                1 -> $pl
                1 -> $pr
                1 -> $ps

                8 -> $state
            }
            8 -> {
                # --- Keyboard loop: U/D/L/R select, SET presses ---
                ram imut $u: u8 = @digital_read_c(0)
                ram imut $d: u8 = @digital_read_c(1)
                ram imut $l: u8 = @digital_read_c(2)
                ram imut $r: u8 = @digital_read_c(3)
                ram imut $set: u8 = @digital_read_d(4)

                ram mut $changed: u8 = 0
                ram mut $next_row: u8 = $sel_row
                ram mut $next_col: u8 = $sel_col

                ? $u == 0 {
                    ? $pu != 0 {
                        ? $sel_row > 0 {
                            $sel_row - 1 -> $next_row
                            1 -> $changed
                        }
                    }
                }
                ? $d == 0 {
                    ? $pd != 0 {
                        ? $sel_row < 2 {
                            $sel_row + 1 -> $next_row
                            1 -> $changed
                        }
                    }
                }
                ? $l == 0 {
                    ? $pl != 0 {
                        ? $sel_col > 0 {
                            $sel_col - 1 -> $next_col
                            1 -> $changed
                        }
                    }
                }
                ? $r == 0 {
                    ? $pr != 0 {
                        ? $sel_col < 9 {
                            $sel_col + 1 -> $next_col
                            1 -> $changed
                        }
                    }
                }

                ? $changed != 0 {
                    @draw_key($sel_row, $sel_col, 0)
                    $next_row -> $sel_row
                    $next_col -> $sel_col
                    @draw_key($sel_row, $sel_col, 1)
                }

                ? $set == 0 {
                    ? $ps != 0 {
                        ram imut $ch: u8 = @get_key_char($sel_row, $sel_col)
                        ? $ch == 8 {
                            ? $n_len > 0 {
                                $n_len - 1 -> $n_len
                                ? $n_len == 0 { 95 -> $n0 }
                                ? $n_len == 1 { 95 -> $n1 }
                                ? $n_len == 2 { 95 -> $n2 }
                                @draw_entered_name($n0, $n1, $n2)
                            }
                        } : {
                            ? $ch == 10 {
                                ? $n_len > 0 {
                                    ? $n_len == 1 {
                                        32 -> $n1
                                        32 -> $n2
                                    }
                                    ? $n_len == 2 {
                                        32 -> $n2
                                    }
                                    @insert_high_score(&$leaderboard[0], $score, $n0, $n1, $n2)
                                    7 -> $state
                                }
                            } : {
                                ? $n_len < 3 {
                                    ? $n_len == 0 { $ch -> $n0 }
                                    ? $n_len == 1 { $ch -> $n1 }
                                    ? $n_len == 2 { $ch -> $n2 }
                                    $n_len + 1 -> $n_len
                                    @draw_entered_name($n0, $n1, $n2)
                                }
                            }
                        }
                    }
                }

                $u -> $pu
                $d -> $pd
                $l -> $pl
                $r -> $pr
                $set -> $ps
                @delay_ms(40)
            }
            7 -> {
                # --- Ranking screen init ---
                @gfx_set_fill(0x00, 0x00)
                @gfx_rect(0, 0, 240, 240)

                @gfx_set_text(0x07, 0xE0, 0x00, 0x00) # Green on Black
                ram str $rk_title = "LEADERBOARD"
                @gfx_text($rk_title, 54, 25) -> $sx

                @gfx_set_text(0xFF, 0xE0, 0x00, 0x00) # Yellow on Black
                ram str $rk_pos1 = "1."
                @gfx_text($rk_pos1, 40, 75) -> $sx
                @draw_rank_slot(&$leaderboard[0], 0, 75)

                @gfx_set_text(0xFF, 0xFF, 0x00, 0x00) # White on Black
                ram str $rk_pos2 = "2."
                @gfx_text($rk_pos2, 40, 115) -> $sx
                @draw_rank_slot(&$leaderboard[0], 1, 115)

                ram str $rk_pos3 = "3."
                @gfx_text($rk_pos3, 40, 155) -> $sx
                @draw_rank_slot(&$leaderboard[0], 2, 155)

                @gfx_set_text(0x07, 0xE0, 0x00, 0x00) # Green
                ram str $rk_hint = "SET = MENU"
                @gfx_text($rk_hint, 60, 205) -> $sx

                1 -> $ps
                9 -> $state
            }
            9 -> {
                # --- Ranking loop: SET returns to menu ---
                ram imut $set3: u8 = @digital_read_d(4)
                ? $set3 == 0 {
                    ? $ps != 0 { 0 -> $state }
                }
                $set3 -> $ps
                @delay_ms(40)
            }
            * -> {
                0 -> $state
            }
        }
    }
}
