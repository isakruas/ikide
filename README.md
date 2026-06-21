# IKIDE

The **IKIDE** is the official Integrated Development Environment (IDE) for the **ik** language. It provides an intuitive, graphical interface equipped with real-time diagnostics, an integrated AVR simulator, and direct microcontroller flashing via `avrdude`.

---

## Getting Started

### Clone with submodules
Clone the repository and initialize submodules in one step:
```bash
git clone --recurse-submodules https://github.com/isakruas/ikide.git
```

If you already cloned the repository without submodules:
```bash
cd ikide
git submodule update --init --recursive
```

After a `git pull`, if submodule pointers have changed:
```bash
git submodule update --recursive
```

### Build the IDE
Build the IDE using Docker (which ensures a reproducible environment without needing Rust installed locally):
```bash
make build
```

### Run the IDE
After building, launch the IDE interface:
```bash
make run
```

### Clean build artifacts
To remove generated build files and clean up the `target` directory:
```bash
make clean
```

---

## Core Features
- **In-process Compiler**: Deeply integrated `ik8b` front-end offering live diagnostics, intelligent autocomplete, and inline syntax error checking.
- **Microcontroller Simulator**: Uses an integrated AVR Virtual Machine to seamlessly run and trace your code without needing physical hardware.
- **Avrdude Integration**: Easily flash your compiled Intel HEX files to connected MCU boards. Flashing preferences (like Programmer, Port, Baudrate, and Fuses) can be dynamically configured in the IDE `Preferences` menu.
- **Smart Target Detection**: The target MCU required by `avrdude` and the simulator is automatically inferred directly from your `ik8b` codebase declarations.
- **Scriptable Test Framework**: Write `tests/*.rhai` files that drive the simulated core and assert on its behaviour, then run them all from **Run → 🧪 Run Tests**. The IDE provides the whole bench (compile, core, the breadboard's device models, injection and capture); you only write the tests. See [Test Framework](#test-framework).

## Test Framework

The IDE can run a suite of tests against your program entirely in-process — the
same machinery the breadboard uses, minus the GUI. Put `.rhai` files in a
`tests/` folder at the root of your workspace and choose **Run → 🧪 Run Tests**;
each file runs against a fresh, isolated bench and the IDE reports PASS/FAIL per
check.

The same suite runs headless from the command line — useful for CI:

```bash
ikide test                 # run ./tests/*.rhai
ikide test path/to/project # run a specific workspace's tests/*.rhai
ikide test --mcu atmega32  # target device for load_hex tests (default: atmega328p)
```

`ikide test` prints the same PASS/FAIL report and exits non-zero on any failure.
Running `ikide` with no command launches the graphical IDE; `ikide help` lists
every command.

A test file has full control of the core through these functions. Every
peripheral is controllable from **both** sides — what the program reads and what
it writes:

| Area     | Drive (into the MCU)              | Observe (out of the MCU)                  |
|----------|-----------------------------------|-------------------------------------------|
| Program  | `build("p.ik")`, `load_hex("p.hex")`, `load_device("name" or "path.rhai")`, `reset()` | `running()`, `cycles()`, `executed()` |
| Run      | `step(n)`, `step_until_uart(text, max)` | —                                   |
| UART     | `uart_send("…")`, `uart_byte(b)`  | `uart_recv()`, `uart_text()`, `uart_clear()` |
| SPI      | `set_spi_miso(b)` (or a device)   | `spi_mosi()`, `spi_miso_in()`             |
| I2C/TWI  | `load_device(...)`                | `twi_data()`                              |
| ADC      | `set_adc(ch, value)`              | `peek(addr)` of the ADC registers         |
| GPIO     | `set_pin(addr, bit, high)`, `clear_pin(addr, bit)` | `pin(addr, bit)`, `peek(addr)` |
| SRAM/IO  | `poke(addr, v)`, `set_reg(i, v)`  | `peek(addr)`, `reg(i)`                     |
| EEPROM   | `eeprom_write(addr, v)`           | `eeprom_read(addr)`                        |
| Interrupt| `irq(vector)` (raise any vector)  | auto-raised from peripheral events        |
| Core     | —                                 | `pc()`, `sp()`, `sreg()`                   |
| Assert   | `check(name, cond)`, `check_eq(name, got, want)`, `check_contains(name, hay, needle)`, `assert(cond, msg)`, `log(msg)` | |

`build(...)` compiles a single ik source in-process and loads it (the target is
taken from the program's own `target` directive). For a multi-file project,
build it first (e.g. `make build`) and use `load_hex("build/your.hex")`.
Relative paths resolve against the workspace root. `spi_mosi()`, `twi_data()`,
`uart_recv()` drain what they return; `uart_text()` is the full accumulated
output.

```rhai
// tests/blinky.rhai — assert the program toggles PORTB0
build("blinky.ik");
step(100000);
check("PB0 driven high", pin(0x25, 0));   // 0x25 = PORTB on the atmega328p
```

For a program that talks over the serial buses, attach the same device models
the breadboard ships and assert on both directions:

```rhai
// tests/spi.rhai
build("driver.ik");
load_device("spi_echo");                // a shipped device, by name (or a path)
set_adc(0, 512);                        // an analog input on ADC channel 0
step_until_uart("ready", 2000000);
check_contains("greeting", uart_text(), "ready");
check("MCU shifted out 0x41", spi_mosi().contains(0x41));
```
