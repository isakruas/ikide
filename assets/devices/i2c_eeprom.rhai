// I2C EEPROM (24C-style) at address 0x50, 256 bytes.
//
// Protocol: after addressing, the first written byte is the memory pointer;
// further writes store data and auto-increment; reads return mem[ptr++].
// This is the classic register-map shape most I2C devices follow — copy it
// and change `meta`/`init`/handlers to model your own sensor.
fn meta() { #{ name: "I2C EEPROM 0x50", bus: "i2c", address: 0x50 } }

fn init() {
    let m = [];
    for i in 0..256 { m.push(0); }
    #{ ptr: 0, mem: m, first: true }
}

fn i2c_address(addr, read) {
    this.first = true;       // next write (if any) is the pointer
    addr == 0x50             // ACK only our address
}

fn i2c_write(byte) {
    if this.first {
        this.ptr = byte;
        this.first = false;
    } else {
        this.mem[this.ptr] = byte;
        this.ptr = (this.ptr + 1) % 256;
    }
    true                     // ACK
}

fn i2c_read(last) {
    let v = this.mem[this.ptr];
    this.ptr = (this.ptr + 1) % 256;
    v
}
