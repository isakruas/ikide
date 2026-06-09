// MAX7219: serially interfaced 8x8 LED display driver.
//
// Data is shifted as 16-bit packets (D15..D8 = register, D7..D0 = data); the
// rising edge of LOAD latches the last 16 bits shifted. Registers 0x01..0x08
// hold the digit (row) data — bit n lights column n. The chip powers up in
// shutdown mode: the display stays blank until the shutdown register (0x0C)
// is written with 1, exactly like the real part.
fn meta() {
    #{
        name: "MAX7219 LED Matrix 8x8",
        bus: "spi",
        pins: #{ load: "PB2" },
        display: #{ w: 8, h: 8 },
    }
}

fn init() {
    let digits = [];
    for i in 0..8 { digits.push(0); }
    // hi/lo model the 16-bit shift register (last two bytes shifted in).
    #{ hi: 0, lo: 0, digits: digits, shutdown: true }
}

// Repaint the panel from the digit registers (blank while in shutdown).
fn redraw(fb, digits, shutdown) {
    for y in 0..8 {
        let row = if shutdown { 0 } else { digits[y] };
        for x in 0..8 {
            let on = ((row >> x) & 1) == 1;
            fb.px(x, y, if on { 0xFF4020 } else { 0 });
        }
    }
}

fn spi_transfer(mosi) {
    this.hi = this.lo;            // 16-bit shift: previous byte moves up
    this.lo = mosi;
    0xFF
}

fn pin_set(name, level) {
    if name != "load" || level != 1 { return; }   // latch on LOAD rising edge

    let reg = this.hi & 0x0F;
    if reg >= 0x01 && reg <= 0x08 {
        this.digits[reg - 1] = this.lo;
    } else if reg == 0x0C {
        this.shutdown = (this.lo & 1) == 0;
    }
    // Decode mode, intensity, scan limit and display test do not change the
    // stored digit data.
    redraw(this.fb, this.digits, this.shutdown);
}
