// PCF8574 — 8-bit I2C GPIO expander at address 0x20 (A0..A2 = 0).
// Writing a byte latches the 8 output pins; reading returns the port. Models
// the quasi-bidirectional port simply as a single latch (pull-ups high).
fn meta() { #{ name: "PCF8574 I/O 0x20", bus: "i2c", address: 0x20 } }
fn init() { #{ port: 0xFF } }

fn i2c_address(addr, read) { addr == 0x20 }
fn i2c_write(byte) { this.port = byte; true }
fn i2c_read(last)   { this.port }
