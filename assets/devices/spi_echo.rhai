// SPI echo device: returns each received byte plus one (mod 256) on MISO.
// A minimal example of a full-duplex SPI responder.
fn meta() { #{ name: "SPI Echo (+1)", bus: "spi" } }
fn spi_transfer(mosi) { (mosi + 1) % 256 }
