# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: 74HC595 Shift Register.
# Shifts a walking bit in over SPI, then pulses the latch (PB2 -> RCLK) to
# copy it to the outputs. Board tab: add the 74HC595 device.

target atmega328p

import std/gpio
import std/spi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @spi_init_master_raw()
    @pin_mode_b(2, 1)        # RCLK (latch)
    @digital_write_b(2, 0)

    ram mut $pat: u8 = 1
    loop * {
        ram mut $r: u8 = 0
        @spi_transfer($pat) -> $r
        @digital_write_b(2, 1)   # rising edge latches the outputs
        @digital_write_b(2, 0)
        $pat * 2 -> $pat
        ? $pat == 0 {
            1 -> $pat
        }
        @delay_ms(150)
    }
}
