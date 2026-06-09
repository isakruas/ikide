# Copyright 2026 The IKIDE Authors.
# SPDX-License-Identifier: Apache-2.0
#
# Breadboard: MPU6050 Reader.
# Wakes the IMU (PWR_MGMT_1 = 0), checks WHO_AM_I, then reads ACCEL_ZOUT_H
# (0x40 = 1 g at rest) and prints both values over UART.
# Board tab: add the MPU6050 device.

target atmega328p

import std/twi
import std/uart
import std/conv
import std/delay

@cpu_mhz() -> u16 {
    return 16
}

# Read one register: set the pointer, repeated start, read with NACK.
@mpu_read($reg: u8) -> u8 {
    @twi_start()
    @twi_write(0xD0)         # 0x68 << 1 | write
    @twi_write($reg)
    @twi_start()
    @twi_write(0xD1)         # 0x68 << 1 | read
    ram imut $v: u8 = @twi_read_nack()
    @twi_stop()
    return $v
}

@main {
    @uart_init(103)
    @twi_init(72)

    # Wake from sleep: PWR_MGMT_1 = 0.
    @twi_start()
    @twi_write(0xD0)
    @twi_write(0x6B)
    @twi_write(0x00)
    @twi_stop()

    ram mut $buf: u8[8] = 0
    loop * {
        ram imut $id: u8 = @mpu_read(0x75)
        ram imut $az: u8 = @mpu_read(0x3F)

        @uart_print_str("who:")
        @utoa($id, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_print_str(" az_h:")
        @utoa($az, &$buf[0])
        @uart_print_str(&$buf[0])
        @uart_println()
        @delay_ms(500)
    }
}
