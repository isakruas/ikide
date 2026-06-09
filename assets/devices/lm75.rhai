// LM75: I2C temperature sensor, address 0x48.
//
// The slider sets the measured temperature. Register 0x00 holds it as two
// bytes: MSB = whole degrees (two's complement), LSB bit 7 = 0.5 °C, exactly
// the 9-bit format the datasheet specifies. The pointer register selects
// which register reads return; configuration (0x01) is stored and read back.
fn meta() {
    #{
        name: "LM75 Temp Sensor 0x48",
        bus: "i2c",
        address: 0x48,
        view: [
            #{ kind: "slider", id: "temp_c", min: -25, max: 100 },
            #{ kind: "text", id: "label" },
        ],
    }
}

fn init() { #{ reg: 0, byte_idx: 0, temp_c: 25.0, config: 0, label: "25.0 °C" } }

fn on_view(id, value) {
    if id == "temp_c" {
        // Quantize to the sensor's 0.5 °C resolution.
        let half = (value * 2.0).round() / 2.0;
        this.temp_c = half;
        this.label = half.to_string() + " °C";
    }
}

fn i2c_address(addr, read) {
    this.byte_idx = 0;
    addr == 0x48
}

fn i2c_write(byte) {
    if this.byte_idx == 0 {
        this.reg = byte;               // pointer register
    } else if this.reg == 0x01 {
        this.config = byte;            // configuration register
    }
    this.byte_idx += 1;
    true
}

fn i2c_read(last) {
    if this.reg == 0x01 {
        return this.config;
    }
    // Temperature register: 9-bit value, MSB then LSB.
    let t2 = (this.temp_c.to_float() * 2.0).to_int();   // halves, signed
    let raw = if t2 < 0 { t2 + 512 } else { t2 };       // two's complement
    let v = if this.byte_idx == 0 {
        (raw >> 1) & 0xFF                               // whole degrees
    } else {
        (raw & 1) * 128                                 // half-degree bit
    };
    this.byte_idx += 1;
    v
}
