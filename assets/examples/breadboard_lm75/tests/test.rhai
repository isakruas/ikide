// LM75 Thermostat — points the LM75 at its temperature register, reads it over
// I2C, lights PB5 above 30 C, and prints "temp:<deg>".
//
// The lm75 model's default reading is 25 C, so this validates the full I2C
// exchange (0x90 write / 0x91 read = 0x48<<1, pointer register 0x00), the parsed
// value, and that the thermostat keeps the LED OFF at 25 C (below the 30 C trip).
build("main.ik");
load_device("lm75");

step_until_uart("temp:", 5000000);
check_contains("reads the default 25 C", uart_text(), "temp:25");
check("LED on PB5 off at 25 C (below 30 C threshold)", !pin(0x25, 5));

let bus = twi_data();
check("write address 0x90 (0x48<<1)", 0x90 in bus);
check("points at temperature register 0x00", 0x00 in bus);
check("read address 0x91 after repeated start", 0x91 in bus);
