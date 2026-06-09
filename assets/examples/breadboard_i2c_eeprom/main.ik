# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: I2C EEPROM.
# Stores a byte in an I2C EEPROM at address 0x50 and reads it back, printing
# the value over UART. On the Breadboard: I2C tab -> attach "I2C EEPROM 0x50";
# watch the bus there and the read-back on the UART tab.

target atmega328p

import std/twi
import std/uart
import std/conv
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @uart_init(103)          # 9600 baud @ 16 MHz
    @twi_init(72)            # ~100 kHz @ 16 MHz

    ram mut $buf: u8[8] = 0
    loop * {
        # Write 0xAB to register 0x10.
        @twi_start()
        @twi_write(0xA0)     # 0x50 << 1 | write
        @twi_write(0x10)     # register pointer
        @twi_write(0xAB)     # data
        @twi_stop()

        # Read it back: set the pointer, repeated start, read with NACK.
        @twi_start()
        @twi_write(0xA0)
        @twi_write(0x10)
        @twi_start()
        @twi_write(0xA1)     # 0x50 << 1 | read
        ram imut $v: u8 = @twi_read_nack()
        @twi_stop()

        @uart_print_str("eeprom[0x10]=")
        @utoa($v, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()

        @delay_ms(500)
    }
}
