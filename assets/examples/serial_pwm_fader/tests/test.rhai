// PWM Duty Fader (atmega32) — sets up Fast-PWM on Timer0 (OC0 = PB3) and sweeps
// the duty cycle up and down, reporting each value as "pwm_duty:<n>".
//
// Validates the init banner, that the duty sweep actually advances (0, then 5,
// 10, ...), and that the hardware compare register OCR0 (data 0x5C) tracks the
// duty written by @pwm0_set_duty_a.
build("main.ik");

step_until_uart("pwm_duty:", 4000000);
let out = uart_text();
check_contains("init banner on OC0/PB3", out, "PWM Fader initialized");
check_contains("duty starts at 0", out, "pwm_duty:0");

// Step further so the sweep climbs; the duty rises by 5 each loop.
step(3000000);
out = uart_text();
check("duty sweeps upward (5 and 10 reported)",
      ("pwm_duty:5" in out) && ("pwm_duty:10" in out));
check("OCR0 (data 0x5C) tracks the swept duty (non-zero)", peek(0x5c) > 0);
