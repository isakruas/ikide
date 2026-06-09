# Virtual Device Authoring Guide

Every part on the IKIDE breadboard — from a bare LED to an SPI display — is a
Rhai script. Drop a `.rhai` file in a `devices/` folder next to your project
(or use **📝 New device script** on the Board tab), press **⟳**, and your
device appears in the catalog. No recompilation.

The promise: model the part you don't have on your bench, debug your program
against the virtual one, then flash the same program to the real board.

## Anatomy of a device

```rhai
// One-line description of the part.
fn meta() {
    #{
        name: "My Sensor",          // shown in the catalog and on the card
        bus: "i2c",                 // none | uart | spi | i2c   (default: none)
        address: 0x42,              // i2c only: the 7-bit address it ACKs
        pins: #{ ... },             // terminals the user wires to MCU pins
        view: [ ... ],              // visual interface, auto-rendered
        display: #{ w: 128, h: 64 } // optional framebuffer (this.fb)
    }
}

fn init() { #{ count: 0 } }         // optional: the initial state (`this`)
```

State persists in `this` between calls. If `init()` is missing, `this` starts
as an empty map.

## Terminals (`pins`)

Each entry names a terminal the user wires to an MCU pin on the device card:

```rhai
pins: #{
    dc:    #{ },                          // watch: observe an MCU output
    irq:   #{ mode: "drive", idle: 1 },   // drive: drive an MCU input
    wiper: #{ mode: "adc" },              // adc:   an ADC channel
    cs:    "PB2",                         // shorthand: watch + default pin
}
```

- **watch** — the script is notified of edges via `pin_set(name, level)`.
- **drive** — the device drives the pin (a button pulling low). `idle` is the
  released level. To drive from script code, set `this.drive = #{ irq: 0 }`.
- **adc** — the terminal selects an ADC channel; pair it with a `slider`.
- `default:` (or the string shorthand) pre-wires the terminal; the user can
  rewire it on the card.

## Visual interface (`view`)

Elements are rendered on the device card, live. Each binds either to a
terminal (**`pin:`** / ordered **`pins:`** array) or to a **state key**
(**`id:`**) that your script updates — letting a peripheral show its own
internal state.

| kind       | shows                          | binds to                   |
|------------|--------------------------------|----------------------------|
| `led`      | one light (`color: 0xRRGGBB`)  | 1 pin, or `id` (0/1)       |
| `rgbled`   | mixed color                    | `pins: [r, g, b]`          |
| `ledbar`   | a row of lights                | `pins: [...]`, or `id` + `bits` (bit n of the value) |
| `sevenseg` | 7-segment digit + dot          | `pins: [a..g, dp]`, or `id` (bit-mapped) |
| `button`   | momentary push button          | 1 drive pin (`press:` level) or `id` (1 held / 0 released) |
| `slider`   | value input (`min:`/`max:`)    | 1 adc pin, or `id`         |
| `text`     | a string from the script       | `id` only                  |

UI events from `id`-bound buttons and sliders reach your script through
`on_view(id, value)` (the value is also stored in `this[id]`).

## Behaviour handlers

All optional — implement only what your part does. Runtime errors abort just
that call, never the simulation.

```rhai
fn pin_set(name, level) { }         // a watched pin changed (0/1)
fn on_view(id, value) { }           // a UI element changed

fn spi_transfer(mosi) { 0xFF }      // full duplex: return the MISO byte

fn i2c_address(addr, read) { addr == 0x42 }  // return true to ACK
fn i2c_write(byte) { true }         // master wrote a byte; true = ACK
fn i2c_read(last) { 0xFF }          // master reads; last = final (NACK) byte
fn i2c_start() { }                  // bus START (also repeated start)
fn i2c_stop() { }                   // bus STOP

fn uart_tx(byte) { }                // the MCU transmitted a byte
fn uart_poll() { () }               // byte to send to the MCU, or () for none

fn tick(cycles) { }                 // simulated time advanced (per frame)
```

## Displays

Declare `display: #{ w, h }` and draw through `this.fb`:

```rhai
this.fb.px(x, y, 0xFF0000);   // set one pixel (0xRRGGBB)
this.fb.fill(0x000000);       // clear
```

The panel renders at the top of the Board tab while the simulation runs.

## Helpers available to scripts

- `chr(code)` — one-character string from an ASCII code (building text views).

## Worked examples (in this folder)

- `led.rhai`, `push_button.rhai` — the smallest possible devices (no handlers).
- `pcf8574.rhai` — I2C device whose LED bar shows script state (`id` binding).
- `at24c256.rhai` — multi-byte protocol state machine (16-bit addressing).
- `st7789.rhai` — SPI display: control pins + command decoding + framebuffer.
- `hd44780_i2c.rhai` — a protocol-in-protocol decoder (LCD behind an I2C
  expander), rendering with a `text` view.

## Execution model and limits

Bus handlers (`spi_transfer`, `i2c_*`, `uart_tx`) run synchronously inside the
instruction that drives the bus, so request/response protocols are exact.
Watched pins are delivered on every PORT register write, also synchronously —
this is what makes command/data and chip-select decoding exact too.

Device-driven outputs (`this.drive`, `uart_poll`, `tick`) are serviced once
per engine frame. Protocols where the *device* must produce pin transitions
with microsecond timing (single-wire sensor handshakes, ultrasonic echo
pulses) therefore cannot be modeled; bus-based parts are unaffected.

Scripts run under an operation cap: a runaway loop aborts that call (logged
to the terminal as `[device NAME] handler(): ...`) instead of freezing the
simulation.
