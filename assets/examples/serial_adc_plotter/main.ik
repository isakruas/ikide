# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Analog ADC Serial Plotter Example.
# Reads analog voltages from two ADC channels (channel 0 and channel 1) and prints them to UART.
# Perfect to visualize analog sensor readings in the Serial Plotter.

target atmega32

import std/adc
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
    
    # Initialize ADC
    @adc_init()

    ram mut $buf: u8[8] = 0

    loop * {
        # Read analog channels (0 and 1)
        ram imut $val0: u16 = @adc_read(0)
        ram imut $val1: u16 = @adc_read(1)

        # Print to UART in plotter format: "ch0:val0,ch1:val1\n"
        @uart_print_str("ch0:")
        @utoa($val0, &$buf[0])
        @uart_print_str(&$buf[0])

        @uart_print_char(44) # ','

        @uart_print_str("ch1:")
        @utoa($val1, &$buf[0])
        @uart_print_str(&$buf[0])

        @uart_println()

        @delay_ms(100)
    }
}
