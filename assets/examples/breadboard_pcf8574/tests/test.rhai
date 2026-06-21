// PCF8574 Port Expander — walks a single set bit across the 8 expander outputs
// by writing successive one-hot bytes over I2C (control 0x40 = 0x20<<1), 150 ms
// apart, wrapping after bit 7.
//
// Validates the I2C addressing and the EXACT walking-bit data sequence written.
build("main.ik");
load_device("pcf8574");

// Collect the data bytes following each control byte over a couple of laps.
let seen = [];
for i in 0..40 {
    step(800000);
    let bus = twi_data();
    for b in bus { if !(b in seen) { seen.push(b); } }
    if (1 in seen) && (2 in seen) && (4 in seen) && (8 in seen)
       && (16 in seen) && (32 in seen) && (64 in seen) && (128 in seen) { break; }
}

check("addresses the PCF8574 (control 0x40)", 0x40 in seen);
let walk = true;
for p in [1, 2, 4, 8, 16, 32, 64, 128] { if !(p in seen) { walk = false; } }
check("writes the full one-hot walk 1,2,4,...,128", walk);
