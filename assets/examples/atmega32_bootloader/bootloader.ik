# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# ============================================================================
# ik serial bootloader for the ATmega32
# ============================================================================
# Lives in the Boot Loader Section and rewrites the *application* section of
# flash with an image streamed over UART by the IDE ("Flash via bootloader").
#
# Fuses required (set once, with avrdude/ISP):
#   - BOOTRST = 0  (reset jumps into the boot section, so this runs on reset)
#   - BOOTSZ  = 2048 words (4 KB) -> boot section starts at byte 0x7000
#
# Wire protocol (host = IDE, target = this bootloader); integers big-endian:
#
#   Frame  host -> target:  0x1B 'i' 'k'  CMD  LEN_HI LEN_LO  payload[LEN]  CRC_HI CRC_LO
#                           CRC-16/ARC (poly 0xA001, init 0) over CMD,LEN_HI,LEN_LO,payload.
#   Reply  target -> host:  0x06 (ACK) or 0x15 (NAK), then command-specific bytes.
#
#   CMD 0x01 HELLO  payload: (none)  -> ACK, VER, PAGE_HI, PAGE_LO, APPEND_HI, APPEND_LO
#   CMD 0x02 WRITE  payload: ADDR_HI ADDR_LO  <PAGE_SIZE data bytes>  -> ACK | NAK
#   CMD 0x04 RUN    payload: (none)  -> ACK, then jump to the application at 0x0000
#
# Security / validation rules (enforced below):
#   - every frame is CRC-16 checked; a bad CRC is NAKed and ignored.
#   - WRITE is rejected (NAK) unless the page address is page-aligned and the
#     whole page lies strictly below the boot section -> the bootloader can
#     never erase/overwrite itself nor write past the end of flash.
#   - interrupts are disabled around every SPM sequence.
#   - with no valid sync inside the startup window, the existing application is
#     started, so a normally-flashed board boots without the IDE.

target atmega32
boot 0x7000

import std/uart
import std/boot
import std/crc

# --- Tunables ---------------------------------------------------------------
# UBRR for the desired baud at the board's clock: UBRR = F_CPU/(16*baud) - 1.
# Default: 8 MHz, 9600 baud -> 51. (1 MHz/9600 -> 6, 16 MHz/9600 -> 103.)
const BL_UBRR: u16 = 51
# Coarse startup window (outer x inner UART polls) before running the app.
const BL_WAIT_OUTER: u16 = 250
const BL_WAIT_INNER: u16 = 2000

# --- Protocol constants -----------------------------------------------------
const SOF0: u8 = 0x1B
const SOF1: u8 = 0x69          # 'i'
const SOF2: u8 = 0x6B          # 'k'
const ACK:  u8 = 0x06
const NAK:  u8 = 0x15
const CMD_HELLO: u8 = 0x01
const CMD_WRITE: u8 = 0x02
const CMD_RUN:   u8 = 0x04
const PROTO_VER: u8 = 1

# ATmega32: 128-byte (64-word) flash page; the application section is
# [0x0000, 0x7000) since the boot section starts at 0x7000.
const PAGE_SIZE: u16 = 128
const PAGE_MASK: u16 = 127
const APP_END:   u16 = 0x7000

# Read `$n` bytes from the UART into the buffer at offset `$off`.
@bl_read_into($buf: ptr ram u8, $off: u16, $n: u16) {
    loop 0..$n -> $i {
        ram imut $b: u8 = @uart_receive()
        $b -> *($buf + $off + $i)
    }
}

# Block until the 3-byte sync magic (0x1B 'i' 'k') arrives. Every frame is
# prefixed with it, so this also resynchronises after any garbled frame.
@bl_resync() {
    ram mut $state: u8 = 0
    loop * {
        ram imut $b: u8 = @uart_receive()
        ? $state == 0 {
            ? $b == SOF0 { 1 -> $state }
        } : {
            ? $state == 1 {
                ? $b == SOF1 { 2 -> $state } : { 0 -> $state }
            } : {
                ? $b == SOF2 { return } : { 0 -> $state }
            }
        }
    }
}

# Wait for the 3-byte sync magic, polling without blocking so we can time out.
# Returns 1 when sync is seen, 0 on timeout.
@bl_wait_sync() -> u8 {
    ram mut $state: u8 = 0
    loop 0..BL_WAIT_OUTER -> $o {
        loop 0..BL_WAIT_INNER -> $j {
            ? @uart_available() != 0 {
                ram imut $b: u8 = @uart_receive()
                ? $state == 0 {
                    ? $b == SOF0 { 1 -> $state }
                } : {
                    ? $state == 1 {
                        ? $b == SOF1 { 2 -> $state } : { 0 -> $state }
                    } : {
                        ? $b == SOF2 { return 1 } : { 0 -> $state }
                    }
                }
            }
        }
    }
    return 0
}

# Program one flash page from buffer[$data_off .. +PAGE_SIZE] at page byte
# address $addr. The caller has already validated $addr.
@bl_write_page($buf: ptr ram u8, $addr: u16, $data_off: u16) {
    @cli()
    @boot_page_erase($addr)
    loop 0..64 -> $k {
        ram imut $lo: u16 = *($buf + $data_off + ($k * 2))
        ram imut $hi: u16 = *($buf + $data_off + ($k * 2) + 1)
        @boot_page_fill($addr + ($k * 2), $lo + ($hi * 256))
    }
    @boot_page_write($addr)
    @boot_rww_enable()
}

# Handle one validated (CRC-checked) frame.
@bl_dispatch($buf: ptr ram u8, $cmd: u8, $len: u16) {
    ? $cmd == CMD_HELLO {
        @uart_send(ACK)
        @uart_send(PROTO_VER)
        @uart_send(PAGE_SIZE / 256)
        @uart_send(PAGE_SIZE & 0xFF)
        @uart_send(APP_END / 256)
        @uart_send(APP_END & 0xFF)
        return
    }
    ? $cmd == CMD_RUN {
        @uart_send(ACK)
        @boot_rww_enable()
        @goto(0)
    }
    ? $cmd == CMD_WRITE {
        ram imut $addr: u16 = (*($buf + 3) * 256) + *($buf + 4)
        # Security: page-aligned, inside the application section, whole page
        # below the boot section, and exactly one page of data.
        ram mut $ok: u8 = 1
        ? ($addr & PAGE_MASK) != 0 { 0 -> $ok }
        ? $addr >= APP_END { 0 -> $ok }
        ? ($addr + PAGE_SIZE) > APP_END { 0 -> $ok }
        ? $len != (PAGE_SIZE + 2) { 0 -> $ok }
        ? $ok == 1 {
            @bl_write_page($buf, $addr, 5)
            @uart_send(ACK)
        } : {
            @uart_send(NAK)
        }
        return
    }
    @uart_send(NAK)
}

@main {
    # Frame scratch: CMD + LEN(2) + payload(<=130) + CRC(2) = 135 bytes.
    ram mut $frame: u8[135] = 0

    @uart_init(BL_UBRR)

    # Startup window: if nobody syncs, run whatever is already in flash.
    ? @bl_wait_sync() == 0 {
        @goto(0)
    }

    loop * {
        # CMD (1) + LEN (2).
        @bl_read_into(&$frame[0], 0, 3)
        ram imut $cmd: u8 = $frame[0]
        ram imut $len: u16 = ($frame[1] * 256) + $frame[2]

        ? $len > 130 {
            # Reject oversized frames (still drain the trailing CRC bytes).
            @bl_read_into(&$frame[0], 3, 2)
            @uart_send(NAK)
        } : {
            # Payload then the 2 CRC bytes, contiguous after CMD/LEN.
            @bl_read_into(&$frame[0], 3, $len + 2)
            ram imut $rx_crc: u16 = ($frame[3 + $len] * 256) + $frame[4 + $len]
            ram imut $calc: u16 = @crc16(&$frame[0], 3 + $len)
            ? $calc != $rx_crc {
                @uart_send(NAK)
            } : {
                @bl_dispatch(&$frame[0], $cmd, $len)
            }
        }
        # Every frame is self-framed: wait for the next one's sync magic.
        @bl_resync()
    }
}
