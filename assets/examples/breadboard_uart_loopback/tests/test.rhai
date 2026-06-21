// UART Loopback — sends 'A'..'Z' over the USART and waits for each byte to come
// back (via the loopback model), toggling PB5 on every verified round trip.
//
// Validates that the full alphabet is transmitted in order and that PB5 (PORTB
// data 0x25, bit 5) actually toggles per round trip.
build("main.ik");
load_device("uart_loopback");

let lo = false;
let hi = false;
for i in 0..30 {
    step(2000000);
    if pin(0x25, 5) { hi = true; } else { lo = true; }
    if "ABCDEFGHIJKLMNOPQRSTUVWXYZ" in uart_text() { break; }
}
let out = uart_text();

check_contains("transmits the alphabet A..Z in order", out, "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
check("PB5 toggles on verified round trips (both levels seen)", lo && hi);
