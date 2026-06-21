// Text on ST7789 — clears the panel to a dark-blue background and renders
// "IKIDE" with the 5x8 std/font glyphs scaled 4x, over SPI.
//
// Validates the windowed-write commands and that BOTH the background (RGB565
// 0x1082) and the white text (0xFFFF) pixels reach the bus. The full-screen
// clear comes first, so we accumulate MOSI until the white glyph pixels appear.
build("main.ik");
load_device("st7789");

let caset = false; let raset = false; let ramwr = false;
let bg_hi = false; let bg_lo = false; let white = false; let total = 0;
for i in 0..80 {
    step(2000000);
    let m = spi_mosi();
    total += m.len();
    if 0x2A in m { caset = true; }
    if 0x2B in m { raset = true; }
    if 0x2C in m { ramwr = true; }
    if 0x10 in m { bg_hi = true; }
    if 0x82 in m { bg_lo = true; }
    if 0xFF in m { white = true; }
    if caset && raset && ramwr && bg_hi && bg_lo && white { break; }
}

check("CASET command 0x2A", caset);
check("RASET command 0x2B", raset);
check("RAMWR command 0x2C", ramwr);
check("background pixel 0x1082 (high 0x10)", bg_hi);
check("background pixel 0x1082 (low 0x82)", bg_lo);
check("white text pixels 0xFFFF (byte 0xFF)", white);
check("streams the cleared frame plus glyphs", total > 10000);
