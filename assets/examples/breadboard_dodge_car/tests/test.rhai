// Dodge Car 3D — a multi-file game (main.ik + gfx.ik + road.ik, merged via
// `import`) rendering to the ST7789 over SPI.
//
// Validates that the whole program compiles and boots, drives the TFT with the
// windowed-write command sequence, and streams a substantial amount of pixel
// data (the menu / first frame) — i.e. it really runs and renders.
build("main.ik");
load_device("st7789");

step(15000000);
let mosi = spi_mosi();

check("boots and drives the ST7789 over SPI", mosi.len() > 1000);
check("CASET (column window) command 0x2A", 0x2A in mosi);
check("RASET (row window) command 0x2B", 0x2B in mosi);
check("RAMWR (pixel write) command 0x2C", 0x2C in mosi);
