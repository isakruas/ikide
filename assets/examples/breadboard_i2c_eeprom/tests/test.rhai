// I2C EEPROM — writes 0xAB to register 0x10 of a 24Cxx and reads it back (171).
//
// Validates the full I2C transaction (control + register pointer + data, then a
// repeated-start read) on the bus, plus the value returned by the model.
build("main.ik");
load_device("i2c_eeprom");

step_until_uart("eeprom[0x10]=", 5000000);
check_contains("reads back 0xAB = 171", uart_text(), "eeprom[0x10]=171");

let bus = twi_data();
check("write control byte 0xA0 (0x50<<1)", 0xA0 in bus);
check("register pointer 0x10", 0x10 in bus);
check("data byte 0xAB written", 0xAB in bus);
check("read control byte 0xA1 after repeated start", 0xA1 in bus);
