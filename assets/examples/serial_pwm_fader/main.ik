# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# PWM Duty Cycle Fader Example.
# Generates a PWM signal on Timer0 with a duty cycle that sweeps up and down,
# outputting the current duty cycle and status to the Serial Monitor.

target atmega32

import std/pwm
import std/uart
import std/delay
import std/conv

# The board's CPU clock, in MHz.
@cpu_mhz() -> u16 {
    return 8
}

@main {
    # Initialize UART (8 MHz, 9600 baud -> UBRR = 51)
    @uart_init(51)
    
    # Initialize Fast PWM on Timer0 (Prescaler 8)
    @pwm0_init_fast(2)
    @pwm0_enable_output_a()

    @uart_print_str("PWM Fader initialized on OC0 (PB3).\n")

    ram mut $duty: u16 = 0
    ram mut $up: u8 = 1
    ram mut $buf: u8[8] = 0

    loop * {
        # Update PWM0 output compare register
        ram imut $d_u8: u8 = $duty
        @pwm0_set_duty_a($d_u8)

        # Output the current duty cycle to the Serial Plotter format
        @uart_print_str("pwm_duty:")
        @utoa($duty, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()

        # Sweep logic
        ? $up == 1 {
            $duty + 5 -> $duty
            ? $duty >= 255 {
                0 -> $up
            }
        } : {
            $duty - 5 -> $duty
            ? $duty <= 0 {
                1 -> $up
            }
        }

        @delay_ms(30)
    }
}
