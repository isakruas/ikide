// AT24C256 EEPROM — writes 0xC3 at the 16-bit address 0x1234 and reads it back.
//
// Validates the complete two-phase I2C protocol the driver emits on the bus
// (twi_data carries every TWDR byte, address + data) AND the value read back
// through the at24c256 model.
build("main.ik");
load_device("at24c256");

step_until_uart("at24[0x1234]=", 5000000);
check_contains("reads back 0xC3 = 195", uart_text(), "at24[0x1234]=195");

let bus = twi_data();
check("write control byte 0xA0 (0x50<<1)", 0xA0 in bus);
check("16-bit word address high 0x12", 0x12 in bus);
check("16-bit word address low 0x34", 0x34 in bus);
check("data byte 0xC3 written", 0xC3 in bus);
check("read control byte 0xA1 after repeated start", 0xA1 in bus);
