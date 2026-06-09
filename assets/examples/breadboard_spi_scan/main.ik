# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: SPI Byte Scan.
# SPI is full duplex: every transfer sends a byte on MOSI and receives one on
# MISO in the same clocks. This sends an incrementing byte and prints what
# came back over UART. Board tab: add the SPI Echo device to see the
# response track the sent byte (+1).

target atmega328p

import std/spi
import std/uart
import std/conv
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @uart_init(103)
    @spi_init_master_raw()

    ram mut $buf: u8[8] = 0
    ram mut $b: u8 = 0
    loop * {
        ram mut $resp: u8 = 0
        @spi_transfer($b) -> $resp

        @uart_print_str("sent:")
        @utoa($b, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_print_str(" got:")
        @utoa($resp, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()

        $b + 1 -> $b
        @delay_ms(100)
    }
}
