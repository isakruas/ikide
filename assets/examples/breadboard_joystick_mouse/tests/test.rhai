// Joystick Mouse on ST7789 — a pointer steered by the joystick (PC0..PC5); the
// scene is procedural and only the patch under the cursor is redrawn.
//
// Validates that it boots and draws the scene over SPI (windowed writes), and
// that moving the joystick triggers a redraw (more pixels go out after a move).
build("main.ik");
load_device("st7789");

for b in 0..6 { set_pin(0x26, b, true); }   // all directions released (pull-ups)
step(8000000);
let first = spi_mosi();
check("draws the initial scene over SPI", first.len() > 1000);
check("CASET command 0x2A", 0x2A in first);
check("RASET command 0x2B", 0x2B in first);
check("RAMWR pixel-write command 0x2C", 0x2C in first);

set_pin(0x26, 0, false);     // press UP (PC0)
step(4000000);
let after = spi_mosi();
check("moving the pointer triggers a redraw (more SPI)", after.len() > 0 && (0x2c in after));
