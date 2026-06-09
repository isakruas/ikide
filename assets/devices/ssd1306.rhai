// SSD1306: 128x64 monochrome OLED, I2C address 0x3C.
//
// Each I2C transaction begins with a control byte: 0x00 = command stream,
// 0x40 = display data. Commands are decoded with their full argument counts;
// the three memory addressing modes are implemented as specified:
//   horizontal (0): column advances through the window, then the page
//   vertical   (1): page advances through the window, then the column
//   page       (2): column advances within the page only (power-on default)
// Data bytes paint 8 vertical pixels of the current page (bit 0 on top).
fn meta() {
    #{
        name: "SSD1306 OLED 128x64",
        bus: "i2c",
        address: 0x3C,
        display: #{ w: 128, h: 64 },
    }
}

fn init() {
    #{
        ctrl: -1,                 // control byte of the current transaction
        pending: 0,               // command-argument bytes still expected
        cmd: 0,
        args: [],
        mode: 2,                  // memory addressing mode (page on reset)
        c0: 0, c1: 127,           // column window (horizontal/vertical modes)
        p0: 0, p1: 7,             // page window
        col: 0, page: 0,          // address pointers
    }
}

// Argument byte count of every command, per the command table.
fn arg_count(cmd) {
    if cmd == 0x20 || cmd == 0x81 || cmd == 0x8D || cmd == 0xA8
        || cmd == 0xD3 || cmd == 0xD5 || cmd == 0xD9 || cmd == 0xDA
        || cmd == 0xDB { return 1; }
    if cmd == 0x21 || cmd == 0x22 || cmd == 0xA3 { return 2; }
    if cmd == 0x26 || cmd == 0x27 { return 6; }     // horizontal scroll setup
    if cmd == 0x29 || cmd == 0x2A { return 5; }     // diagonal scroll setup
    0
}

fn i2c_address(addr, read) {
    this.ctrl = -1;               // a new transaction selects command or data
    addr == 0x3C
}

fn i2c_write(byte) {
    if this.ctrl < 0 {
        this.ctrl = byte;
        return true;
    }

    if this.ctrl == 0x40 {
        // Display data: 8 vertical pixels, then advance per addressing mode.
        for b in 0..8 {
            let on = ((byte >> b) & 1) == 1;
            this.fb.px(this.col, this.page * 8 + b, if on { 0xE0F0FF } else { 0 });
        }
        if this.mode == 0 {                   // horizontal
            this.col += 1;
            if this.col > this.c1 {
                this.col = this.c0;
                this.page += 1;
                if this.page > this.p1 { this.page = this.p0; }
            }
        } else if this.mode == 1 {            // vertical
            this.page += 1;
            if this.page > this.p1 {
                this.page = this.p0;
                this.col += 1;
                if this.col > this.c1 { this.col = this.c0; }
            }
        } else {                              // page: column wraps in place
            this.col = (this.col + 1) % 128;
        }
        return true;
    }

    // Command stream.
    if this.pending > 0 {
        this.args.push(byte);
        this.pending -= 1;
        if this.pending == 0 {
            let c = this.cmd;
            if c == 0x20 {
                this.mode = this.args[0] & 0x03;
            } else if c == 0x21 {
                this.c0 = this.args[0]; this.c1 = this.args[1];
                this.col = this.c0;
            } else if c == 0x22 {
                this.p0 = this.args[0]; this.p1 = this.args[1];
                this.page = this.p0;
            }
            // Contrast, charge pump, scroll, hardware-config arguments are
            // consumed here; they do not change the pixel RAM.
        }
        return true;
    }

    if byte >= 0xB0 && byte <= 0xB7 {         // page start (page mode)
        this.page = byte - 0xB0;
    } else if byte <= 0x0F {                  // column low nibble (page mode)
        this.col = (this.col & 0xF0) | byte;
    } else if byte >= 0x10 && byte <= 0x1F {  // column high nibble (page mode)
        this.col = (this.col & 0x0F) | ((byte - 0x10) * 16);
    } else {
        this.cmd = byte;
        this.args = [];
        this.pending = arg_count(byte);
    }
    true
}
