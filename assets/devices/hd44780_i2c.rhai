// HD44780 16x2 character LCD behind a PCF8574 I2C backpack at 0x27.
//
// Backpack wiring: P0=RS, P1=RW, P2=EN, P3=backlight, P4..P7=data nibble.
// The host pulses EN high then low for each nibble; the controller latches on
// the falling edge. After the function-set nibble 0x2 switches the interface
// to 4-bit mode, high+low nibble pairs form bytes: RS=0 commands, RS=1 data.
//
// DDRAM is modeled at its full 80 bytes — row 0 at addresses 0x00..0x27,
// row 1 at 0x40..0x67 — and the 16x2 window shows the first 16 of each row,
// as on the real module. Address writes auto-increment (the power-on entry
// mode); clear (0x01) and return home (0x02/0x03) reset the cursor.
fn meta() {
    #{
        name: "LCD 16x2 I2C (HD44780)",
        bus: "i2c",
        address: 0x27,
        view: [#{ kind: "text", id: "screen" }],
    }
}

fn init() {
    let ddram = blob();
    ddram.pad(80, 0x20);
    #{
        ddram: ddram, ac: 0,          // address counter (DDRAM address)
        four_bit: false, have_hi: false, hi: 0,
        en: 0, screen: "",
    }
}

// Render the visible 16x2 window: row 0 = DDRAM 0x00.., row 1 = 0x40..
fn render(ddram) {
    let s = "";
    for i in 0..16 { s += chr(ddram[i]); }
    s += "\n";
    for i in 0..16 { s += chr(ddram[0x28 + i]); }
    s
}

// DDRAM address -> index in the 80-byte store (row 1 starts at offset 0x28).
fn cell_index(ac) {
    if ac >= 0x40 { 0x28 + (ac - 0x40) } else { ac }
}

// Auto-increment per the controller: row 0 runs 0x00..0x27, row 1 0x40..0x67.
fn next_ac(ac) {
    if ac == 0x27 { return 0x40; }
    if ac == 0x67 { return 0x00; }
    ac + 1
}

fn i2c_address(addr, read) { addr == 0x27 }

fn i2c_write(byte) {
    let en = (byte >> 2) & 1;
    if this.en == 1 && en == 0 {          // EN falling edge latches the nibble
        let nibble = (byte >> 4) & 0x0F;
        let rs = byte & 1;
        if !this.four_bit {
            // Still in 8-bit mode: the function-set nibble 0x2 switches over.
            if nibble == 0x2 { this.four_bit = true; }
        } else if !this.have_hi {
            this.hi = nibble;
            this.have_hi = true;
        } else {
            this.have_hi = false;
            let full = (this.hi << 4) | nibble;
            if rs == 1 {                  // data: store and advance
                let idx = cell_index(this.ac);
                if idx < 80 { this.ddram[idx] = full; }
                this.ac = next_ac(this.ac);
                this.screen = render(this.ddram);
            } else if full == 0x01 {      // clear display
                for i in 0..80 { this.ddram[i] = 0x20; }
                this.ac = 0;
                this.screen = render(this.ddram);
            } else if full == 0x02 || full == 0x03 {  // return home
                this.ac = 0;
            } else if full >= 0x80 {      // set DDRAM address
                this.ac = full - 0x80;
            }
            // Function set, display control and entry mode (the default
            // increment mode is modeled) require no further state.
        }
    }
    this.en = en;
    true
}
