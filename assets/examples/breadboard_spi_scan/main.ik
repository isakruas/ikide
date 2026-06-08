# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: SPI Byte Scan.
# Initializes SPI as master and transmits an incrementing byte every 100 ms.
# Watch the bytes in the Breadboard's SPI tab; set a MISO response there to
# observe what the master reads back.

target atmega328p

import std/spi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @spi_init_master_raw()
    ram mut $b: u8 = 0
    loop * {
        ram imut $resp: u8 = @spi_transfer($b)
        $b + 1 -> $b
        @delay_ms(100)
    }
}
