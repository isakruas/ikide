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
