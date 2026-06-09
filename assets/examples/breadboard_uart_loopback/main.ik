# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: UART Loopback.
# Sends 'A'..'Z' over the USART and waits for each byte to come back,
# toggling PB5 on every verified round trip.
# Board tab: add the UART Loopback device and an LED wired to PB5.

target atmega328p

import std/gpio
import std/uart
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

@main {
    @uart_init(103)
    @pin_mode_b(5, 1)
    loop * {
        loop 65..91 -> $c {
            @uart_send($c)
            ram imut $r: u8 = @uart_receive()
            ? $r == $c {
                @toggle_b(5)
            }
            @delay_ms(100)
        }
    }
}
