# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Serial Plotter Test Application.
# Generates a sine and an inverted sine wave signals (opposite phases) and prints them to UART.
# Designed to be loaded and run in the simulator to test the Serial Plotter tab.

target atmega32

import std/uart
import std/delay
import std/conv
import std/math

# The board's CPU clock, in MHz.
@cpu_mhz() -> u16 {
    return 8
}

# Helper to print a null-terminated string over UART.
@uart_print_str($s: str ram) {
    ram mut $i: u16 = 0
    loop * {
        ram imut $c: u8 = *($s + $i)
        ? $c == 0 { return }
        @uart_send($c)
        $i + 1 -> $i
    }
}

@main {
    # Initialize UART (8 MHz, 9600 baud -> UBRR = 51)
    @uart_init(51)

    ram mut $deg: u16 = 0
    ram mut $buf: u8[8] = 0

    loop * {
        ram imut $s: r16 = @sin($deg)

        # Scale Q8.8 value from [-1, 1] to [5, 95]: (val * 45) + 50
        ram imut $s_scaled: r16 = @_q88_mul($s, 45.0) + 50.0
        # Invert the phase to make them completely opposite: 100 - val
        ram imut $c_scaled: r16 = 100.0 - $s_scaled

        # Extract integer part by assigning to u16 and dividing by 256
        ram imut $s_raw: u16 = $s_scaled
        ram imut $c_raw: u16 = $c_scaled
        ram imut $s_val: u16 = $s_raw / 256
        ram imut $c_val: u16 = $c_raw / 256

        # Print "sin:val"
        @uart_print_str("sin:")
        @utoa($s_val, &$buf[0])
        @uart_print_str(&$buf[0])

        # Print separator comma
        @uart_print_char(44) # ','

        # Print "cos:val" (using cos label to keep same channels)
        @uart_print_str("cos:")
        @utoa($c_val, &$buf[0])
        @uart_print_str(&$buf[0])

        # End of sample line
        @uart_println()

        # Update angle
        $deg + 5 -> $deg
        ? $deg >= 360 {
            0 -> $deg
        }

        @delay_ms(50)
    }
}
