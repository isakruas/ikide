# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: Blink.
# Toggles an LED on PB5 every 500 ms. Add an LED on the Breadboard wired to
# PB5, set the Clock to 16 MHz, and press Run.

target atmega328p

import std/gpio
import std/delay

# The board's CPU clock, in MHz. Match the Breadboard "Clock" setting.
@cpu_mhz() -> u16 {
    return 16
}

@main {
    @pin_mode_b(5, 1)        # PB5 as output
    loop * {
        @toggle_b(5)         # flip the LED
        @delay_ms(500)
    }
}
