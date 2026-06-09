# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: LM75 Thermostat.
# Reads the LM75 temperature register (MSB = whole degrees) and lights PB5
# when the reading exceeds 30 C, printing each reading over UART.
# Board tab: add the LM75 device (drag its slider) and an LED on PB5.

target atmega328p

import std/twi
import std/gpio
import std/uart
import std/conv
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @uart_init(103)
    @twi_init(72)
    @pin_mode_b(5, 1)

    # Point at the temperature register once.
    @twi_start()
    @twi_write(0x90)         # 0x48 << 1 | write
    @twi_write(0x00)
    @twi_stop()

    ram mut $buf: u8[8] = 0
    loop * {
        @twi_start()
        @twi_write(0x91)     # 0x48 << 1 | read
        ram imut $deg: u8 = @twi_read_ack()
        ram imut $frac: u8 = @twi_read_nack()
        @twi_stop()

        ? $deg > 30 {
            @digital_write_b(5, 1)
        } : {
            @digital_write_b(5, 0)
        }

        @uart_print_str("temp:")
        @utoa($deg, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()
        @delay_ms(250)
    }
}
