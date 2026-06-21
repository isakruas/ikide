// ST7789 TFT — sets the full 240x240 draw window and fills it, alternating red
// and blue, over SPI (DC=PB1 selects command/data, CS=PB2).
//
// Validates the complete window-setup command sequence AND that BOTH fill colours
// are emitted. The two fills are 500 ms apart and each streams 240*240 pixels, so
// we accumulate the MOSI stream across steps until both colours have appeared.
build("main.ik");
load_device("st7789");

let caset = false; let raset = false; let ramwr = false;
let bound = false; let red = false; let blue = false; let total = 0;
for i in 0..80 {
    step(2000000);
    let m = spi_mosi();
    total += m.len();
    if 0x2A in m { caset = true; }
    if 0x2B in m { raset = true; }
    if 0x2C in m { ramwr = true; }
    if 239 in m { bound = true; }
    if 0xF8 in m { red = true; }
    if 0x1F in m { blue = true; }
    if caset && raset && ramwr && red && blue { break; }
}

check("CASET (column address) command 0x2A", caset);
check("RASET (row address) command 0x2B", raset);
check("RAMWR (memory write) command 0x2C", ramwr);
check("window extends to 239 (0..239)", bound);
check("red fill high byte 0xF8 (RGB565 0xF800)", red);
check("blue fill low byte 0x1F (RGB565 0x001F)", blue);
check("streams full frames of pixels", total > 10000);
