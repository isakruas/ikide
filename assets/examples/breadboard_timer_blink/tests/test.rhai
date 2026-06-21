// Timer Interrupt Blinker (atmega32) — a Timer1 overflow ISR counts overflows
// and toggles PB0, logging each count over UART; main only prints when the
// count changes.
//
// Validates the start-up banner, that the overflow ISR fires REPEATEDLY (the
// count reaches 1 then 2), and that PB0 (PORTB data 0x38, bit 0) toggles with it.
build("main.ik");

step_until_uart("starting", 3000000);
check_contains("prints the start-up banner", uart_text(), "Timer Interrupt Blinker starting");

step_until_uart("Timer Overflows: 1", 12000000);
check_contains("first Timer1 overflow logged", uart_text(), "Timer Overflows: 1");

// PB0 toggles in the ISR — sample across more overflows to see both levels.
let lo = false;
let hi = false;
for i in 0..12 {
    step(3000000);
    if pin(0x38, 0) { hi = true; } else { lo = true; }
}
check_contains("overflow count keeps incrementing (reaches 2)", uart_text(), "Timer Overflows: 2");
check("PB0 toggles from the Timer1 ISR", lo && hi);
