// ST7789: 240x240 SPI TFT. Watches the D/C and CS pins and decodes the
// command stream — CASET/RASET set the draw window, RAMWR streams RGB565
// pixels into the framebuffer (`this.fb`). The template for SPI displays.
fn meta() {
    #{
        name: "ST7789 TFT 240x240",
        bus: "spi",
        pins: #{ dc: "PB1", cs: "PB2" },
        display: #{ w: 240, h: 240 },
    }
}

fn init() {
    #{
        dc: 1, cs: 1, cmd: 0,
        x0: 0, x1: 239, y0: 0, y1: 239,
        cx: 0, cy: 0,
        args: [], hi: -1,
    }
}

fn pin_set(name, level) {
    if name == "dc" { this.dc = level; }
    if name == "cs" { this.cs = level; }
}

fn spi_transfer(mosi) {
    if this.cs == 1 { return 0xFF; }       // not selected

    if this.dc == 0 {                       // command byte
        this.cmd = mosi;
        this.args.clear();
        this.hi = -1;
        if mosi == 0x2C {                   // RAMWR: cursor to window origin
            this.cx = this.x0;
            this.cy = this.y0;
        }
        return 0xFF;
    }

    let c = this.cmd;
    if c == 0x2A {                          // CASET: x0, x1 (16-bit each)
        this.args.push(mosi);
        if this.args.len() == 4 {
            this.x0 = (this.args[0] << 8) | this.args[1];
            this.x1 = (this.args[2] << 8) | this.args[3];
        }
    } else if c == 0x2B {                   // RASET: y0, y1
        this.args.push(mosi);
        if this.args.len() == 4 {
            this.y0 = (this.args[0] << 8) | this.args[1];
            this.y1 = (this.args[2] << 8) | this.args[3];
        }
    } else if c == 0x2C {                   // RAMWR: RGB565, 2 bytes/pixel
        if this.hi < 0 {
            this.hi = mosi;
        } else {
            let px = (this.hi << 8) | mosi;
            this.hi = -1;
            // RGB565 -> RGB888, expanding each channel to full range so
            // 0xFFFF renders as true white.
            let r = ((px >> 11) & 0x1F) * 255 / 31;
            let g = ((px >> 5) & 0x3F) * 255 / 63;
            let b = (px & 0x1F) * 255 / 31;
            this.fb.px(this.cx, this.cy, (r << 16) | (g << 8) | b);
            this.cx += 1;
            if this.cx > this.x1 {
                this.cx = this.x0;
                this.cy += 1;
                if this.cy > this.y1 { this.cy = this.y0; }
            }
        }
    }
    return 0xFF;
}
