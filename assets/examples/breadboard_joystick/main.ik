# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: Joystick Cursor.
# A 5-way joystick moves a lit cursor across an LED bar: L/R step the
# position, RST recenters it, SET fills the whole bar while held, and every
# press is logged over UART. The six lines are active-low with pull-ups.
# Board tab: add the Joystick (defaults PC0..PC5) and an LED bar on PD0..PD7.

target atmega328p

import std/gpio
import std/uart
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

# Bitmask for cursor index 0..7.
@cursor_mask($idx: u8) -> u8 {
    ram mut $mask: u8 = 1
    loop 0..$idx -> $i {
        $mask * 2 -> $mask
    }
    return $mask
}

@main {
    @uart_init(103)
    0xFF -> %DDRD            # LED bar on PORTD

    # Joystick lines as inputs with pull-ups (read 0 while pressed).
    loop 0..6 -> $p {
        @pin_mode_c($p, 0)
        @digital_write_c($p, 1)
    }

    ram mut $idx: u8 = 3     # cursor position
    ram mut $pu: u8 = 1      # previous levels, for press-edge detection
    ram mut $pd: u8 = 1
    ram mut $pl: u8 = 1
    ram mut $pr: u8 = 1
    ram mut $prst: u8 = 1

    loop * {
        ram imut $u: u8 = @digital_read_c(0)
        ram imut $d: u8 = @digital_read_c(1)
        ram imut $l: u8 = @digital_read_c(2)
        ram imut $r: u8 = @digital_read_c(3)
        ram imut $set: u8 = @digital_read_c(4)
        ram imut $rst: u8 = @digital_read_c(5)

        ? $u == 0 {
            ? $pu != 0 { @uart_print_str("U\n") }
        }
        ? $d == 0 {
            ? $pd != 0 { @uart_print_str("D\n") }
        }
        ? $l == 0 {
            ? $pl != 0 {
                @uart_print_str("L\n")
                ? $idx != 7 { $idx + 1 -> $idx }
            }
        }
        ? $r == 0 {
            ? $pr != 0 {
                @uart_print_str("R\n")
                ? $idx != 0 { $idx - 1 -> $idx }
            }
        }
        ? $rst == 0 {
            ? $prst != 0 {
                @uart_print_str("RST\n")
                3 -> $idx
            }
        }

        # SET fills the bar while held; otherwise show the cursor bit.
        ? $set == 0 {
            0xFF -> %PORTD
        } : {
            ram mut $mask: u8 = 0
            @cursor_mask($idx) -> $mask
            $mask -> %PORTD
        }

        $u -> $pu
        $d -> $pd
        $l -> $pl
        $r -> $pr
        $rst -> $prst
        @delay_ms(30)
    }
}
