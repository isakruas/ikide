# I2C EEPROM

Writes a byte to an I2C EEPROM at **0x50** and reads it back over TWI, printing the result via UART.

## On the breadboard

- On the **Board** tab, add the **I2C EEPROM 0x50** device.

## Run

Run, watch the bus decode on the **I2C** tab ([S]/addr/data/[P]) and the read-back on the **UART** tab.
