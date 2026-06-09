// AT24C256: 32 KB I2C EEPROM at 0x50 with a 16-bit memory address
// (high byte, low byte). Storage is sparse; unwritten cells read 0xFF.
fn meta() { #{ name: "AT24C256 EEPROM 0x50", bus: "i2c", address: 0x50 } }
fn init() { #{ ptr: 0, mem: #{}, need: 0 } }

fn i2c_address(addr, read) {
    if !read {
        this.need = 2;                 // a write starts with 2 address bytes
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
    let v = this.mem[this.ptr.to_string()];   // missing key reads as 0xFF
    this.ptr = (this.ptr + 1) % 32768;
    v
}
