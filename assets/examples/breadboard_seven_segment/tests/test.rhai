// 7-Segment Counter — counts 0..9 on a common-cathode digit wired a..g = PD0..PD6.
//
// The program writes the segment bitmask for each digit to PORTD (data 0x2B),
// holds 700 ms, then advances, wrapping 9 -> 0. This test verifies the EXACT
// segment pattern for every digit, in order, and the wrap.
build("main.ik");

// Canonical a..g (bit0..bit6) masks for digits 0..9.
let digit = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];

// Collect each new PORTD pattern (one per change) for a full count + the wrap.
let seq = [];
let last = -1;
for i in 0..220 {
    step(400000);
    let v = peek(0x2b);
    if v != last { seq.push(v); last = v; }
    if seq.len() >= 11 { break; }
}

check("captured all 10 digits plus the wrap", seq.len() >= 11);
let ok = seq.len() >= 11;
for i in 0..10 {
    if i >= seq.len() || seq[i] != digit[i] { ok = false; }
}
check("shows the correct segment mask for digits 0..9 in order", ok);
check("wraps from 9 (0x6F) back to 0 (0x3F)", seq.len() >= 11 && seq[10] == 0x3F);
