// EEPROM Configuration Database (atmega32) — writes 42, 84, 126, 168, 210 to
// on-chip EEPROM addresses 0x10..0x14, then reads them back and dumps each.
//
// Validates the start-up/progress banners and that EVERY written value is read
// back correctly (round-trip through the modeled EEPROM), plus completion.
build("main.ik");

step_until_uart("completed", 6000000);
let out = uart_text();

check_contains("start-up banner", out, "EEPROM Database Test starting");
check_contains("write phase announced", out, "Writing values to EEPROM");
check_contains("read phase announced", out, "Reading values back");
check_contains("reads back 42",  out, "= 42");
check_contains("reads back 84",  out, "= 84");
check_contains("reads back 126", out, "= 126");
check_contains("reads back 168", out, "= 168");
check_contains("reads back 210", out, "= 210");
check_contains("reports completion", out, "EEPROM operations completed");
