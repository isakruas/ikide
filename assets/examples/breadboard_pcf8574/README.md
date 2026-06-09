# PCF8574 Port Expander

Walks a single set bit across the expander's eight outputs by writing the
port byte over I2C.

## On the breadboard

- Add the **PCF8574 I/O 0x20** device.

## Run

Run and watch the device card: its LED bar mirrors the latched outputs as
the bit walks. The I2C tab shows each `[S] addr 0x20W data [P]` transaction.
