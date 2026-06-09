// 74HC595: 8-bit serial-in, parallel-out shift register with output latch.
//
// Bytes shift in over SPI (SER/SRCLK); a rising edge on the latch terminal
// (RCLK) copies the shift register to the outputs, shown on the LED bar.
fn meta() {
    #{
        name: "74HC595 Shift Register",
        bus: "spi",
        pins: #{ latch: "PB2" },
        view: [#{ kind: "ledbar", id: "q", bits: 8, color: 0x52A0FF }],
    }
}

fn init() { #{ shift: 0, q: 0 } }

fn spi_transfer(mosi) {
    this.shift = mosi;
    0xFF
}

fn pin_set(name, level) {
    if name == "latch" && level == 1 {    // RCLK rising edge
        this.q = this.shift;
    }
}
