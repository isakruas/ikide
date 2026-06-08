# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Multiphase Waveform Generator Example.
# Generates four distinct signals (sine, cosine, triangle, and sawtooth waves) and outputs them to the Serial Plotter.

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
    ram mut $triangle: u16 = 0
    ram mut $tri_up: u8 = 1
    ram mut $sawtooth: u16 = 0
    ram mut $buf: u8[8] = 0

    loop * {
        # 1. Sine Wave
        ram imut $s: r16 = @sin($deg)
        ram imut $s_scaled: r16 = @_q88_mul($s, 45.0) + 50.0
        ram imut $s_raw: u16 = $s_scaled
        ram imut $s_val: u16 = $s_raw / 256

        # 2. Cosine Wave (Inverted Sine)
        ram imut $c_scaled: r16 = 100.0 - $s_scaled
        ram imut $c_raw: u16 = $c_scaled
        ram imut $c_val: u16 = $c_raw / 256

        # 3. Triangle Wave (0 to 100)
        ram imut $t_val: u16 = $triangle

        # 4. Sawtooth Wave (0 to 100)
        ram imut $saw_val: u16 = $sawtooth

        # Print "sine:val,cosine:val,triangle:val,sawtooth:val\n"
        @uart_print_str("sine:")
        @utoa($s_val, &$buf[0])
        @uart_print_str(&$buf[0])

        @uart_print_char(44) # ','

        @uart_print_str("cosine:")
        @utoa($c_val, &$buf[0])
        @uart_print_str(&$buf[0])

        @uart_print_char(44) # ','

        @uart_print_str("triangle:")
        @utoa($t_val, &$buf[0])
        @uart_print_str(&$buf[0])

        @uart_print_char(44) # ','

        @uart_print_str("sawtooth:")
        @utoa($saw_val, &$buf[0])
        @uart_print_str(&$buf[0])

        @uart_println()

        # Update degrees
        $deg + 6 -> $deg
        ? $deg >= 360 { 0 -> $deg }

        # Update triangle
        ? $tri_up == 1 {
            $triangle + 5 -> $triangle
            ? $triangle >= 100 { 0 -> $tri_up }
        } : {
            $triangle - 5 -> $triangle
            ? $triangle <= 0 { 1 -> $tri_up }
        }

        # Update sawtooth
        $sawtooth + 4 -> $sawtooth
        ? $sawtooth >= 100 { 0 -> $sawtooth }

        @delay_ms(40)
    }
}
