# AT24C256 EEPROM

Writes `0xC3` at the 16-bit address `0x1234`, reads it back with a repeated
start, and prints the value (195) over UART.

## On the breadboard

- Add the **AT24C256 EEPROM 0x50** device.

## Run

Run and open the UART tab: `at24[0x1234]=195` prints every half second. The
I2C tab shows the two-byte addressing on the bus.
