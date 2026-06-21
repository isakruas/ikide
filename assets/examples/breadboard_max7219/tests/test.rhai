// MAX7219 LED Matrix — sends 16-bit (register, data) frames over SPI, each
// latched by a LOAD (PB2) rising edge: wake from shutdown, then write the eight
// row registers to draw an "X".
//
// Validates that EVERY expected (register, data) frame appears as an adjacent
// MOSI byte pair, in the right order.
build("main.ik");
load_device("max7219");

step(4000000);
let mosi = spi_mosi();

// Is byte `a` immediately followed by byte `b` anywhere in the stream?
fn pair_at(arr, a, b) {
    let n = arr.len();
    let i = 0;
    let found = false;
    while i < n - 1 {
        if arr[i] == a && arr[i + 1] == b { found = true; break; }
        i += 1;
    }
    found
}

check("wake from shutdown: reg 0x0C = 0x01", pair_at(mosi, 0x0C, 0x01));
check("row 1 = 0x81", pair_at(mosi, 0x01, 0x81));
check("row 2 = 0x42", pair_at(mosi, 0x02, 0x42));
check("row 3 = 0x24", pair_at(mosi, 0x03, 0x24));
check("row 4 = 0x18", pair_at(mosi, 0x04, 0x18));
check("row 5 = 0x18", pair_at(mosi, 0x05, 0x18));
check("row 6 = 0x24", pair_at(mosi, 0x06, 0x24));
check("row 7 = 0x42", pair_at(mosi, 0x07, 0x42));
check("row 8 = 0x81", pair_at(mosi, 0x08, 0x81));
