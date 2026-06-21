// SSD1306 OLED — initialises the panel in horizontal addressing mode and streams
// a 128x64 checkerboard over I2C (control 0x78 = 0x3C<<1).
//
// Validates the full init command set and the framebuffer data stream on the bus.
build("main.ik");
load_device("ssd1306");

step(5000000);
let bus = twi_data();

check("addresses the OLED (0x78 = 0x3C<<1)", 0x78 in bus);
check("display-on command 0xAF", 0xAF in bus);
check("memory addressing-mode command 0x20", 0x20 in bus);
check("column-range command 0x21", 0x21 in bus);
check("page-range command 0x22", 0x22 in bus);
check("column window upper bound 127", 127 in bus);
check("page window upper bound 7", 7 in bus);
check("data control byte 0x40", 0x40 in bus);
check("checkerboard data 0xAA", 0xAA in bus);
check("checkerboard data 0x55", 0x55 in bus);
