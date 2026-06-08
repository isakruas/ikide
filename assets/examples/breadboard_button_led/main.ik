# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: Button + LED.
# Lights an LED on PB5 while a button on PD2 is pressed. The button uses the
# internal pull-up, so the pin reads low (0) only while held.

target atmega328p

import std/gpio

@main {
    @pin_mode_b(5, 1)        # PB5 = LED output
    @pin_mode_d(2, 0)        # PD2 = button input
    @digital_write_d(2, 1)   # enable the internal pull-up on PD2

    loop * {
        ram mut $b: u8 = 0
        @digital_read_d(2) -> $b
        ? $b == 0 {                  # active-low: 0 means pressed
            @digital_write_b(5, 1)   # LED on
        } : {
            @digital_write_b(5, 0)   # LED off
        }
    }
}
