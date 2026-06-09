# Text on ST7789 (std/font)

Renders `IKIDE` on the TFT using the 5x8 glyph table from `std/font`,
scaled 4x. Each character cell is one 24x32 draw window; for every glyph
row, `@font_get_col()` supplies the column bytes (bit 0 = top row, column 5
is the inter-character spacing).

## On the breadboard

- Add the **ST7789 TFT 240x240** device (DC=PB1, CS=PB2).

## Run

Set the Clock to 16 MHz and press Run: the panel clears to dark blue and the
white text appears centered. The same loops drive a real module unchanged —
only the panel is virtual.
