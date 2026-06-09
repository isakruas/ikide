// AT24C256: 256-Kbit (32768 x 8) serial EEPROM, I2C address 0x50.
//
// A write transaction carries a 16-bit memory address (high byte first) and
// then data. Per the datasheet, writes roll over within the 64-byte page;
// sequential reads roll over the entire array. Erased cells read 0xFF.
fn meta() { #{ name: "AT24C256 EEPROM 0x50", bus: "i2c", address: 0x50 } }

fn init() {
    let m = blob();
    m.pad(32768, 0xFF);
    #{ ptr: 0, mem: m, need: 0 }
}

fn i2c_address(addr, read) {
    if !read {
        this.need = 2;                 // the two address bytes come first
        this.ptr = 0;
    }
    addr == 0x50
}

fn i2c_write(byte) {
    if this.need > 0 {
        this.ptr = ((this.ptr << 8) | byte) & 0x7FFF;
        this.need -= 1;
    } else {
        this.mem[this.ptr] = byte;
        // Page write: the address wraps within the current 64-byte page.
        let page = this.ptr - (this.ptr % 64);
        this.ptr = page + ((this.ptr + 1) % 64);
    }
    true
}

fn i2c_read(last) {
    let v = this.mem[this.ptr];
    this.ptr = (this.ptr + 1) % 32768; // sequential read rolls over the array
    v
}
