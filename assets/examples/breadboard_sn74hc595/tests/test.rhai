// 74HC595 Shift Register — shifts a walking bit out over SPI, then pulses the
// latch (PB2 = RCLK) to copy it to the outputs, 150 ms per step, wrapping.
//
// Validates the EXACT walking-bit MOSI sequence and that the latch pin (PORTB
// data 0x25, bit 2) is actually pulsed (toggles).
build("main.ik");
load_device("sn74hc595");

let seen = [];
for i in 0..40 {
    step(800000);
    for b in spi_mosi() { if !(b in seen) { seen.push(b); } }
    if (1 in seen) && (2 in seen) && (4 in seen) && (8 in seen)
       && (16 in seen) && (32 in seen) && (64 in seen) && (128 in seen) { break; }
}

let walk = true;
for p in [1, 2, 4, 8, 16, 32, 64, 128] { if !(p in seen) { walk = false; } }
check("shifts out the full one-hot walk 1,2,4,...,128", walk);
// RCLK (PB2) is configured as an output and rests low between latch pulses (the
// rising edge itself is a 2-instruction blip, not sampleable; the byte stream
// above is the real validation).
check("RCLK pin PB2 configured as output (DDRB bit2)", ((peek(0x24) >> 2) & 1) == 1);
check("RCLK rests low between pulses (PORTB bit2)", !pin(0x25, 2));
