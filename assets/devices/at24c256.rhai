// AT24C256 — 32 KB I2C EEPROM at address 0x50.
//
// Protocol (per the avr-modular-system driver): a write transaction sends a
// 16-bit memory address (high byte then low byte) before any data; data
// writes auto-increment; reads return mem[ptr++]. Storage is sparse so the
// full 32 KB needs no up-front allocation.
fn meta() { #{ name: "AT24C256 EEPROM 0x50", bus: "i2c", address: 0x50 } }
fn init() { #{ ptr: 0, mem: #{}, need: 0 } }

fn i2c_address(addr, read) {
    if !read {
        // A write begins with the two address bytes; reads reuse the pointer.
        this.need = 2;
        this.ptr = 0;
    }
    addr == 0x50
}

fn i2c_write(byte) {
    if this.need > 0 {
        this.ptr = ((this.ptr << 8) | byte) & 0x7FFF;
        this.need -= 1;
    } else {
        this.mem[this.ptr.to_string()] = byte;
        this.ptr = (this.ptr + 1) % 32768;
    }
    true
}

fn i2c_read(last) {
    // A never-written cell indexes to (), which the host reads as 0xFF — the
    // natural "erased" EEPROM value.
    let v = this.mem[this.ptr.to_string()];
    this.ptr = (this.ptr + 1) % 32768;
    v
}
