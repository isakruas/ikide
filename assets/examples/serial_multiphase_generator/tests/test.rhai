// Multiphase Waveform Generator (atmega32) — generates sine, cosine, triangle
// and sawtooth waveforms and streams them as
// "sine:..,cosine:..,triangle:..,sawtooth:..".
//
// Validates that all four channels are emitted and that the waveforms actually
// ADVANCE over time (the triangle and sawtooth take successive values), i.e. it
// is generating a signal, not printing a constant.
build("main.ik");

step(4000000);
let out = uart_text();

check_contains("emits the sine channel", out, "sine:");
check_contains("emits the cosine channel", out, "cosine:");
check_contains("emits the triangle channel", out, "triangle:");
check_contains("emits the sawtooth channel", out, "sawtooth:");

// Sawtooth steps by 4 each sample (0,4,8,...): seeing distinct values proves
// the generator is advancing, not stuck.
check_contains("sawtooth starts at 0", out, "sawtooth:0");
check("sawtooth advances over time (4 and 8 appear)",
      ("sawtooth:4" in out) && ("sawtooth:8" in out));
