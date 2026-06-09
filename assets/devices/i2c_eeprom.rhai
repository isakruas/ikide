// I2C EEPROM (24C-style) at 0x50, 256 bytes, 8-bit memory pointer.
// The classic register-map shape — copy this to model most I2C chips.
fn meta() { #{ name: "I2C EEPROM 0x50", bus: "i2c", address: 0x50 } }

fn init() {
    let m = [];
    for i in 0..256 { m.push(0); }
    #{ ptr: 0, mem: m, first: true }
}

fn i2c_address(addr, read) {
    this.first = true;                 // next write is the pointer
    addr == 0x50
}

fn i2c_write(byte) {
    if this.first {
        this.ptr = byte;
        this.first = false;
    } else {
        this.mem[this.ptr] = byte;
        this.ptr = (this.ptr + 1) % 256;
    }
    true
}

fn i2c_read(last) {
    let v = this.mem[this.ptr];
    this.ptr = (this.ptr + 1) % 256;
    v
}
