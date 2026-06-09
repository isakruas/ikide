# SPI Byte Scan

SPI is full duplex: every transfer clocks a byte out on MOSI and a byte in on
MISO simultaneously. This sends an incrementing byte and prints both sides of
each exchange over UART.

## On the breadboard

- Optional: on the **Board** tab, add the **SPI Echo (+1)** device.

## Run

Run and open the UART tab: `sent:N got:M` prints each exchange. With the
echo device attached, `got` is always `sent + 1`; without it, `got` is the
configured MISO fallback. The SPI tab shows the same traffic as `MOSI→MISO`
pairs.
