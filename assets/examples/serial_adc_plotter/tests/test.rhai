// ADC Serial Plotter (atmega32) — reads ADC channels 0 and 1 and prints them in
// the Serial-Plotter format "ch0:<v0>,ch1:<v1>".
//
// Validates that BOTH channels are read independently and printed with their
// injected values, in the exact plotter format.
build("main.ik");

set_adc(0, 333);
set_adc(1, 777);
step(3000000);
let out = uart_text();

check_contains("emits the exact plotter line ch0:333,ch1:777", out, "ch0:333,ch1:777");
check_contains("channel 0 reads its injected value", out, "ch0:333");
check_contains("channel 1 reads its own injected value (independent)", out, "ch1:777");
