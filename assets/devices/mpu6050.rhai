// MPU6050: 6-axis accelerometer/gyroscope, I2C address 0x68.
//
// A full register file backs reads and writes (so configuration written by a
// driver reads back), with the identity and sensor-output registers serving
// datasheet values for a device at rest: 1 g on the Z axis (±2g scale,
// 0x4000), ~25 °C, zero rotation. Burst reads auto-increment the pointer.
fn meta() {
    #{
        name: "MPU6050 IMU 0x68",
        bus: "i2c",
        address: 0x68,
    }
}

fn init() {
    let regs = blob();
    regs.pad(256, 0);
    regs[0x75] = 0x68;            // WHO_AM_I
    regs[0x3F] = 0x40;            // ACCEL_ZOUT_H: 0x4000 = 1 g at ±2g
    regs[0x41] = 0x14;            // TEMP_OUT_H } 0x1490 ≈ 25 °C with the
    regs[0x42] = 0x90;            // TEMP_OUT_L } datasheet temperature formula
    regs[0x6B] = 0x40;            // PWR_MGMT_1 resets with SLEEP set
    #{ regs: regs, reg: 0, first: true }
}

fn i2c_address(addr, read) {
    if !read { this.first = true; }    // a write begins with the pointer
    addr == 0x68
}

fn i2c_write(byte) {
    if this.first {
        this.reg = byte;
        this.first = false;
    } else {
        this.regs[this.reg] = byte;    // configuration writes are stored
        this.reg = (this.reg + 1) % 256;
    }
    true
}

fn i2c_read(last) {
    let v = this.regs[this.reg];
    this.reg = (this.reg + 1) % 256;
    v
}
