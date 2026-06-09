# MPU6050 Reader

Wakes the IMU from its power-on sleep, verifies the identity register
(`who:104` = 0x68) and reads the Z-axis acceleration high byte
(`az_h:64` = 0x40, i.e. 1 g at rest).

## On the breadboard

- Add the **MPU6050 IMU 0x68** device.

## Run

Run and open the UART tab: both values print every half second. The I2C tab
shows the wake write and the pointer/repeated-start read pattern.
