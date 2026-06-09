# ST7789 Display

Drives an ST7789 SPI TFT: sets the full 240×240 draw window and fills it,
alternating red and blue. Demonstrates a device that uses both SPI **and**
control pins (D/C, CS).

## On the breadboard

- On the **Board** tab, add the **ST7789 TFT 240x240** device.
- It watches **DC = PB1** and **CS = PB2** (the SPI hardware pins are
  MOSI = PB3, SCK = PB5). Rewire them on the device card if needed.

## Run

Set the Clock to 16 MHz and press Run. The rendered panel appears at the top
of the **Board** tab, filling with red then blue.
