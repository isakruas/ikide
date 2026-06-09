// 24C02-style serial EEPROM: 256 x 8, I2C address 0x50, 8-bit word address.
//
// A write transaction carries the word address and then data; the address
// auto-increments. Reads return mem[address++]. This single-pointer register
// shape is the template for most small I2C memories and sensors.
fn meta() { #{ name: "I2C EEPROM 0x50", bus: "i2c", address: 0x50 } }

fn init() {
    let m = blob();
    m.pad(256, 0xFF);                  // erased cells read 0xFF
    #{ ptr: 0, mem: m, first: true }
}

fn i2c_address(addr, read) {
    this.first = true;                 // a write starts with the word address
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
