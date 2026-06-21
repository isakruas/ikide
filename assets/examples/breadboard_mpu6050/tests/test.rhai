// MPU6050 IMU — wakes the sensor (PWR_MGMT_1 = 0), then reads WHO_AM_I and
// ACCEL_ZOUT_H over I2C and prints both.
//
// Validates the I2C addressing (0xD0 write / 0xD1 read = 0x68<<1), the wake
// write to register 0x6B, the register pointers, and the exact values the model
// returns: WHO_AM_I = 0x68 (104), ACCEL_ZOUT_H = 0x40 (64, ~1 g at rest).
build("main.ik");
load_device("mpu6050");

step_until_uart("who:", 5000000);
let out = uart_text();
check_contains("WHO_AM_I = 104", out, "who:104");
check_contains("ACCEL_ZOUT_H = 64", out, "az_h:64");

let bus = twi_data();
check("write address 0xD0 (0x68<<1)", 0xD0 in bus);
check("read address 0xD1", 0xD1 in bus);
check("wakes via PWR_MGMT_1 (reg 0x6B)", 0x6B in bus);
check("reads WHO_AM_I register 0x75", 0x75 in bus);
check("reads ACCEL_ZOUT_H register 0x3F", 0x3F in bus);
