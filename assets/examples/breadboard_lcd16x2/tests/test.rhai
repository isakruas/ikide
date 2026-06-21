// LCD 16x2 (HD44780 + PCF8574 I2C backpack at 0x27) — drives the LCD in 4-bit
// mode: each byte is sent as two nibbles on P4..P7 with an EN (P2) pulse, RS on
// P0, backlight (P3) kept on; writes the 4-bit init then "HELLO" / "IKIDE".
//
// Validates the I2C addressing and that the 4-bit init and RS=1 data nibbles
// (with the EN pulse) are emitted on the bus.
build("main.ik");
load_device("hd44780_i2c");

step(5000000);
let bus = twi_data();

check("addresses the PCF8574 backpack (0x4E = 0x27<<1)", 0x4E in bus);
// Function-set / 4-bit-switch nibble 0x20 with backlight, EN-low frame = 0x28.
check("4-bit init function-set nibble (0x28)", 0x28 in bus);
// EN-high strobe of that nibble (0x28 | 0x04 = 0x2C).
check("EN pulse strobed (0x2C)", 0x2C in bus);
// 'H' (0x48) high nibble as data: 0x40 | RS(0x01) | backlight(0x08) = 0x49.
check("RS=1 data nibble for a character (0x49)", 0x49 in bus);
check("emits the full init + text as many frames", bus.len() > 40);
