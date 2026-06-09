# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: AT24C256 EEPROM.
# Writes a byte at 16-bit address 0x1234 and reads it back, printing the
# value over UART. Board tab: add the AT24C256 device.

target atmega328p

import std/twi
import std/uart
import std/conv
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @uart_init(103)
    @twi_init(72)

    ram mut $buf: u8[8] = 0
    loop * {
        # Write 0xC3 at 0x1234 (address high byte, low byte, then data).
        @twi_start()
        @twi_write(0xA0)     # 0x50 << 1 | write
        @twi_write(0x12)
        @twi_write(0x34)
        @twi_write(0xC3)
        @twi_stop()

        # Set the address again, repeated start, read one byte.
        @twi_start()
        @twi_write(0xA0)
        @twi_write(0x12)
        @twi_write(0x34)
        @twi_start()
        @twi_write(0xA1)     # 0x50 << 1 | read
        ram imut $v: u8 = @twi_read_nack()
        @twi_stop()

        @uart_print_str("at24[0x1234]=")
        @utoa($v, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()
        @delay_ms(500)
    }
}
