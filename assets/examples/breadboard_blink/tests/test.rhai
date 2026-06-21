// Blink — toggles an LED on PB5 every 500 ms at 16 MHz (@cpu_mhz() = 16).
//
// This validates not just that PB5 toggles, but that it toggles at the right
// RATE: each half-period is delay_ms(500) = 500 ms * 16 MHz = 8,000,000 cycles.
// We measure the cycle gap between consecutive edges on PB5 (data 0x25, bit 5).
build("main.ik");

// Run until PB5 flips; return the cycle count elapsed.
fn wait_edge() {
    let p0 = pin(0x25, 5);
    let c0 = cycles();
    let guard = 0;
    while pin(0x25, 5) == p0 && running() && guard < 400 {
        step(50000);
        guard += 1;
    }
    cycles() - c0
}

wait_edge();                 // discard the partial first interval
let lo = pin(0x25, 5);
let g1 = wait_edge();        // one full half-period
let hi = pin(0x25, 5);
let g2 = wait_edge();        // the next half-period

check("PB5 actually toggles", lo != hi);
// ~8,000,000 cycles per half-period; allow generous slack for loop overhead.
check("two edges measured", g1 > 0 && g2 > 0);
check("half-period ~500 ms (7..9 Mcyc)", g1 > 7000000 && g1 < 9000000);
check("period is stable across toggles", g2 > 7000000 && g2 < 9000000);
