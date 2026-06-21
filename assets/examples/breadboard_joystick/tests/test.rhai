// Joystick Cursor — a 5-way joystick (PC0..PC5, active-low with pull-ups) moves
// a lit cursor across an LED bar on PORTD; L/R step, RST recenters (index 3),
// SET fills the whole bar while held, and every press is logged over UART.
//
// Validates the press logging AND the cursor position on PORTD (data 0x2B) for
// each action — the actual application behaviour, not just "a byte changed".
build("main.ik");
for b in 0..6 { set_pin(0x26, b, true); }   // release all lines
step(1500000);

// Press one PCx line low (others stay released), let the 30 ms loop react.
fn tap(bit) {
    set_pin(0x26, bit, false);
    step(1500000);
    set_pin(0x26, bit, true);
    step(1500000);
}

check("cursor starts centered at index 3 (PORTD = 0x08)", peek(0x2b) == 0x08);

uart_clear();
tap(2);   // LEFT  -> logs "L", index 3 -> 4 (cursor 0x10)
check_contains("LEFT logged", uart_text(), "L");
check("LEFT moved the cursor to index 4 (0x10)", peek(0x2b) == 0x10);

uart_clear();
tap(3);   // RIGHT -> logs "R", index 4 -> 3 (cursor 0x08)
check_contains("RIGHT logged", uart_text(), "R");
check("RIGHT moved the cursor back to index 3 (0x08)", peek(0x2b) == 0x08);

uart_clear();
tap(0);   // UP -> logs "U" (does not move the cursor)
check_contains("UP logged", uart_text(), "U");

uart_clear();
tap(5);   // RST -> logs "RST", recenters to index 3
check_contains("RST logged", uart_text(), "RST");
check("RST recenters the cursor to index 3 (0x08)", peek(0x2b) == 0x08);

// SET fills the whole bar while held.
set_pin(0x26, 4, false);
step(1500000);
check("SET fills the whole bar (PORTD = 0xFF)", peek(0x2b) == 0xFF);
set_pin(0x26, 4, true);
step(1500000);
check("releasing SET restores the cursor (0x08)", peek(0x2b) == 0x08);
