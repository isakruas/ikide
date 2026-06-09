# MAX7219 LED Matrix

Wakes the driver from its power-on shutdown state, then writes the eight row
registers to draw an X on the matrix.

## On the breadboard

- Add the **MAX7219 LED Matrix 8x8** device (LOAD terminal pre-wired to
  **PB2**).

## Run

Run — the 8×8 panel shows the X. Comment out the wake frame (`0x0C, 0x01`)
and the panel stays dark: the model powers up in shutdown like the real chip.
