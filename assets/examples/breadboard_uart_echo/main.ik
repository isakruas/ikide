# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: UART Echo.
# Sends a greeting, then echoes back every received byte. Use the Breadboard's
# UART tab: the greeting appears in the console, and bytes typed in the Send
# box are echoed straight back.

target atmega328p

import std/uart

@main {
    @uart_init(103)              # 16 MHz, 9600 baud -> UBRR = 103
    @uart_print_str("Echo ready\n")
    loop * {
        ram imut $c: u8 = @uart_receive()
        @uart_send($c)
    }
}
