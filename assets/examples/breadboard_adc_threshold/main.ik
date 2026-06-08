# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: ADC Threshold.
# Reads ADC channel 0 (a potentiometer), lights an LED on PB5 above mid-scale,
# and prints the reading over UART. On the Breadboard add a Potentiometer on
# channel 0 and an LED on PB5; the UART tab shows the live value.

target atmega328p

import std/adc
import std/gpio
import std/uart
import std/conv
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @adc_init()
    @uart_init(103)
    @pin_mode_b(5, 1)        # PB5 = LED output

    ram mut $buf: u8[8] = 0
    loop * {
        ram imut $v: u16 = @adc_read(0)
        ? $v > 512 {
            @digital_write_b(5, 1)
        } : {
            @digital_write_b(5, 0)
        }
        @uart_print_str("adc:")
        @utoa($v, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()
        @delay_ms(200)
    }
}
