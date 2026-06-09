# ST7789 Display

Drives an ST7789 SPI TFT: sets a 64×64 draw window and fills it, alternating
red and blue every 500 ms. Demonstrates a device that uses both SPI **and**
control pins (D/C, CS).

## On the breadboard

- On the **SPI** tab, attach the **ST7789 TFT 240x240** device.
- It watches **DC = PB1** and **CS = PB2** (the SPI hardware pins are
  MOSI = PB3, SCK = PB5). Override the pins in the device's `meta()` if your
  wiring differs.

## Run

Set the Clock to 16 MHz and press Run. The rendered framebuffer appears under
**Displays** in the **Schematic** tab, filling with red then blue.
