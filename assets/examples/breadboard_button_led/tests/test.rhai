// Button + LED — an LED on PB5 mirrors a button on PD2 (active-low, internal
// pull-up). The loop is tight (no delay), so the LED follows the pin immediately.
//
// This drives PD2 (PIND data 0x29, bit 2) through several press/release cycles
// and verifies the LED on PB5 (PORTB data 0x25, bit 5) tracks it every time:
// released (high) -> off, pressed (low) -> on.
build("main.ik");

// Helper: set PD2 level and let the tight loop react; return the LED state.
fn set_button_pressed(pressed) {
    set_pin(0x29, 2, !pressed);   // pressed => drive low
    step(100000);
    pin(0x25, 5)                  // resulting LED state
}

let ok = true;
// Exercise multiple cycles to prove it genuinely follows, not a one-off.
for i in 0..3 {
    if set_button_pressed(false) { ok = false; }   // released -> LED off
    if !set_button_pressed(true) { ok = false; }   // pressed  -> LED on
}
check("LED off on release and on while pressed, across cycles", ok);
check("final release leaves LED off", !set_button_pressed(false));
