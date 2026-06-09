# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: LCD 16x2 (HD44780 + I2C backpack).
# Drives the LCD in 4-bit mode through the PCF8574 backpack at 0x27:
# each nibble is presented on P4..P7 with an EN (P2) pulse; RS is P0.
# Board tab: add the LCD 16x2 I2C device; its card shows the text.

target atmega328p

import std/twi
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

# Send one nibble (already in the high half of $n) with an EN pulse.
@lcd_nibble($n: u8, $rs: u8) {
    ram imut $base: u8 = $n | $rs | 0x08     # keep backlight (P3) on
    @twi_start()
    @twi_write(0x4E)         # 0x27 << 1 | write
    @twi_write($base | 0x04) # EN high
    @twi_write($base)        # EN low: the controller latches the nibble
    @twi_stop()
}

# Send a full byte as two nibbles. $rs: 0 = command, 1 = data.
@lcd_send($b: u8, $rs: u8) {
    @lcd_nibble($b & 0xF0, $rs)
    ram imut $low: u8 = ($b & 0x0F) * 16
    @lcd_nibble($low, $rs)
}

@main {
    @twi_init(72)
    @delay_ms(50)            # power-on settle

    @lcd_nibble(0x20, 0)     # function set: switch to 4-bit interface
    @lcd_send(0x28, 0)       # function set: 4-bit, 2 lines
    @lcd_send(0x0C, 0)       # display on, cursor off
    @lcd_send(0x01, 0)       # clear
    @delay_ms(2)

    # Line 1: "HELLO"
    @lcd_send(72, 1)         # H
    @lcd_send(69, 1)         # E
    @lcd_send(76, 1)         # L
    @lcd_send(76, 1)         # L
    @lcd_send(79, 1)         # O

    # Line 2 (DDRAM 0x40): "IKIDE"
    @lcd_send(0xC0, 0)
    @lcd_send(73, 1)         # I
    @lcd_send(75, 1)         # K
    @lcd_send(73, 1)         # I
    @lcd_send(68, 1)         # D
    @lcd_send(69, 1)         # E

    loop * {
        @delay_ms(1000)
    }
}
