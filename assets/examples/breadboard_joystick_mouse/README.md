# Joystick Mouse on ST7789

A miniature pointer-driven GUI, fully simulated: the joystick moves an arrow
cursor over a desktop with three target squares.

- **U / L / R / D** — move the pointer (keeps moving while held)
- **SET** — click: paints the target under the tip with the pointer color;
  inside the menu, picks the pointer color instead
- **RST** — open/close the color menu (red / green / blue bars)

The scene is procedural — every pixel's color comes from `@scene_id()` — so
erasing the pointer is just redrawing the 8x8 patch beneath it. No frame
buffer is needed, which is the point: 240x240x2 bytes would never fit in
2 KB of SRAM.

## On the breadboard

- Add the **ST7789 TFT 240x240** device (DC=PB1, CS=PB2).
- Add the **Joystick (5-way + SET/RST)** device (defaults PC0..PC5).

## Run

Set the Clock to 16 MHz and press Run. The desktop draws (a few seconds of
simulated SPI traffic), then drive the arrow with the joystick: click targets
to repaint them, open the menu with RST and pick a new pointer color.
