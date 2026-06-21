// Knight Rider — a single lit LED walks across PD0..PD7 and wraps.
//
// The program sets all of PORTD to outputs and writes a one-hot mask that
// shifts left every 80 ms, restarting at PD0 after PD7. This test reads PORTD
// (data 0x2B) and verifies the EXACT one-hot sequence and the wrap-around —
// not just "something changed".
build("main.ik");

// Record each new value PORTD takes (ignoring the initial 0 before the first
// write), one entry per change, until we have a full lap plus the wrap.
let seq = [];
let last = 0;
for i in 0..120 {
    step(400000);
    let v = peek(0x2b);
    if v != 0 && v != last { seq.push(v); last = v; }
    if seq.len() >= 9 { break; }
}

check_eq("captured a full lap (8) plus the wrap", seq.len(), 9);
// One-hot, shifting left: 1,2,4,8,16,32,64,128, then back to 1.
let expect = [1, 2, 4, 8, 16, 32, 64, 128, 1];
let ok = seq.len() == 9;
for i in 0..expect.len() {
    if i >= seq.len() || seq[i] != expect[i] { ok = false; }
}
check("walks 1,2,4,8,16,32,64,128 then wraps to 1", ok);
