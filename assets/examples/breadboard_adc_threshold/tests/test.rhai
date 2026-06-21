// ADC Threshold — reads ADC channel 0, lights an LED on PB5 when the reading is
// strictly above mid-scale (512), and prints "adc:<value>" each loop.
//
// This injects several analog values and verifies BOTH outputs every time: the
// exact printed value and the LED state on PB5 (PORTB data 0x25, bit 5), proving
// the comparison boundary (512 = off, 513 = on).
build("main.ik");

// Inject a value, wait for the next reading line, return [printed_ok, led_on].
fn probe(v) {
    uart_clear();
    set_adc(0, v);
    step_until_uart("adc:", 8000000);
    let out = uart_text();
    [("adc:" + v) in out, pin(0x25, 5)]
}

let r0 = probe(0);      check("reads 0",      r0[0]); check("LED off at 0",      !r0[1]);
let r1 = probe(512);    check("reads 512",    r1[0]); check("LED off at 512",    !r1[1]);
let r2 = probe(513);    check("reads 513",    r2[0]); check("LED on just above threshold", r2[1]);
let r3 = probe(1023);   check("reads 1023",   r3[0]); check("LED on at full scale", r3[1]);
