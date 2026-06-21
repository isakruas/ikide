// SPI Byte Scan — sends an incrementing byte over SPI every 100 ms and prints
// what came back. The spi_echo model returns mosi+1, so each line reads
// "sent:N got:N+1".
//
// Validates several consecutive transfers (value relationship and the actual
// MOSI bytes), not just the first one.
build("main.ik");
load_device("spi_echo");

// Collect a few full lines and the bytes the MCU shifted out.
let mosi = [];
for i in 0..40 {
    step(800000);
    for b in spi_mosi() { mosi.push(b); }
    if "got:3" in uart_text() { break; }   // seen at least bytes 0..3
}
let out = uart_text();

check_contains("byte 0 -> echo 1", out, "sent:0 got:1");
check_contains("byte 1 -> echo 2", out, "sent:1 got:2");
check_contains("byte 2 -> echo 3", out, "sent:2 got:3");
check("MOSI carried the incrementing bytes 0,1,2", (0 in mosi) && (1 in mosi) && (2 in mosi));
