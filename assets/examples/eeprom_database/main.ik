# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# EEPROM Configuration Database Example.
# Demonstrates reading and writing user configuration data from the EEPROM.
# Shows memory logging and dumps over the Serial Monitor.

target atmega32

import std/eeprom
import std/uart
import std/delay
import std/conv

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
    ram mut $buf: u8[8] = 0

    @uart_print_str("EEPROM Database Test starting...\n")

    # Write data to EEPROM at addresses 0x0010 - 0x0014
    @uart_print_str("Writing values to EEPROM...\n")
    @eeprom_write(0x0010, 42)
    @eeprom_write(0x0011, 84)
    @eeprom_write(0x0012, 126)
    @eeprom_write(0x0013, 168)
    @eeprom_write(0x0014, 210)

    @delay_ms(10)

    # Read back and print
    @uart_print_str("Reading values back from EEPROM:\n")
    
    ram mut $addr: u16 = 0x0010
    loop * {
        ram imut $val: u8 = @eeprom_read($addr)
        
        @uart_print_str("Addr [0x")
        # Print address
        ram imut $addr_val: u16 = $addr
        @utoa($addr_val, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_print_str("] = ")
        
        # Print value
        ram imut $val_u16: u16 = $val
        @utoa($val_u16, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()

        $addr + 1 -> $addr
        ? $addr > 0x0014 {
            break
        }
    }

    @uart_print_str("EEPROM operations completed.\n")
    loop * {}
}
