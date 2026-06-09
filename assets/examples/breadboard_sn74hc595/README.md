# 74HC595 Shift Register

Shifts a walking-bit pattern in over SPI and pulses RCLK to latch it to the
outputs.

## On the breadboard

- Add the **74HC595 Shift Register** device (latch terminal pre-wired to
  **PB2**).

## Run

Run — the device's LED bar walks. Note the outputs only change on the latch
pulse, not while bits shift in: that is the latch doing its job.
