// PCF8574: 8-bit I2C GPIO expander at 0x20.
// Writes latch the port; reads return it. The LED bar shows the latched
// outputs live — an example of a view bound to script state (`id`).
fn meta() {
    #{
        name: "PCF8574 I/O 0x20",
        bus: "i2c",
        address: 0x20,
        view: [#{ kind: "ledbar", id: "port", bits: 8, color: 0xFFD24D }],
    }
}

fn init() { #{ port: 0xFF } }

fn i2c_address(addr, read) { addr == 0x20 }
fn i2c_write(byte) { this.port = byte; true }
fn i2c_read(last) { this.port }
