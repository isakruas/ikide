// RGB LED Cycle — steps an RGB LED (r=PB1, g=PB2, b=PB3) through all 8 on/off
// colour combinations, 400 ms each, then repeats.
//
// On iteration i (0..8) the program drives PB1=i&1, PB2=i&2, PB3=i&4. This test
// reads PORTB (data 0x25), isolates bits 1..3, and verifies that ALL eight
// combinations appear, in the natural 0..7 order, and that the cycle repeats.
build("main.ik");

let seq = [];
let last = -1;
for i in 0..200 {
    step(800000);
    let v = (peek(0x25) >> 1) & 0x07;   // PB1..PB3 -> 0..7
    if v != last { seq.push(v); last = v; }
    if seq.len() >= 9 { break; }
}

check("captured all 8 colours plus the repeat", seq.len() >= 9);
let ok = seq.len() >= 9;
for i in 0..8 {
    if i >= seq.len() || seq[i] != i { ok = false; }   // 0,1,2,3,4,5,6,7
}
check("cycles colour combinations 0..7 in order", ok);
check("repeats the cycle (8th step back to 0)", seq.len() >= 9 && seq[8] == 0);
