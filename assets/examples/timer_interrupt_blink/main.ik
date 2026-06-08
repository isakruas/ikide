# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Timer Interrupt Blinker Example.
# Uses Timer1 overflow interrupt to toggle a pin (PB0) at periodic intervals without blocking CPU execution.
# Logs events via UART.

target atmega32

import std/gpio
import std/uart
import std/conv
import std/delay

# Constants for shared variables in RAM (safe area at the end of ATmega32 SRAM)
const TIMER1_TICKS_ADDR: u16 = 0x0800
const STATE_ADDR: u16 = 0x0802

# Hardware Registers for Timer1 Configuration on ATmega32
const %TIMSK: u16 = 0x0059
const %TCCR1B: u16 = 0x004E

isr TIMER1_OVF {
    ram ptr u16 $timer1_ticks_ptr = TIMER1_TICKS_ADDR
    ram ptr u8 $state_ptr = STATE_ADDR

    ram imut $ticks: u16 = *$timer1_ticks_ptr + 1
    $ticks -> *$timer1_ticks_ptr
    
    # Toggle PB0 (LED) status
    ram imut $st: u8 = *$state_ptr
    ? $st == 0 {
        1 -> *$state_ptr
        # Turn PB0 high (using PORTB value)
        ram imut $pb_val: u8 = %PORTB | 1
        $pb_val -> %PORTB
    } : {
        0 -> *$state_ptr
        # Turn PB0 low
        ram imut $pb_val: u8 = %PORTB & 0xFE
        $pb_val -> %PORTB
    }
}

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
    
    # Set PB0 as output
    ram imut $ddrb_val: u8 = %DDRB | 1
    $ddrb_val -> %DDRB

    @uart_print_str("Timer Interrupt Blinker starting...\n")

    # Timer1 Configuration: Prescaler 64
    # At 8 MHz, 16-bit timer overflows every: 65536 * 64 / 8_000_000 = 0.524 seconds.
    
    # Enable Timer1 Overflow Interrupt: TOIE1 (bit 2) in TIMSK (0x39)
    ram imut $timsk_val: u8 = %TIMSK | 4
    $timsk_val -> %TIMSK
    
    # Start Timer1 with Prescaler 64 (CS11 and CS10 bits set in TCCR1B -> 3)
    3 -> %TCCR1B

    # Initialize shared variables in RAM via pointers
    ram ptr u16 $timer1_ticks_ptr = TIMER1_TICKS_ADDR
    ram ptr u8 $state_ptr = STATE_ADDR
    0 -> *$timer1_ticks_ptr
    0 -> *$state_ptr

    # Enable interrupts globally
    @sei()

    ram mut $last_tick: u16 = 0
    ram mut $buf: u8[8] = 0

    loop * {
        # Check if ticks changed
        ram imut $current_ticks: u16 = *$timer1_ticks_ptr
        ? $current_ticks != $last_tick {
            $current_ticks -> $last_tick
            
            @uart_print_str("Timer Overflows: ")
            @utoa($current_ticks, &$buf[0])
            @uart_print_str(&$buf[0])
            @uart_println()
        }
        
        @delay_ms(10)
    }
}
