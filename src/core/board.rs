// Copyright 2026 The IKIDE Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Device GPIO map for the breadboard simulator.
//
// The single source of truth for every chip's port layout is `std/gpio.ik`,
// which carries one `? target == <device> { ... }` block per supported part
// with the data-space addresses of its PIN/DDR/PORT registers. Rather than
// duplicate that table in Rust, we parse the embedded std source for the
// active target — so the breadboard stays correct for all 350 devices without
// a second table to keep in sync.

use crate::core::analysis;

/// One I/O port (a letter, e.g. `B`) with the data-space addresses of its
/// three registers. A pin `n` of this port is bit `n` of each register.
#[derive(Clone, Debug)]
pub struct PortReg {
    pub letter: char,
    pub pin_addr: u32,
    pub ddr_addr: u32,
    pub port_addr: u32,
}

/// A single physical GPIO pin (e.g. `PB5`), resolved to the register addresses
/// and bit the breadboard reads/writes.
#[derive(Clone, Debug)]
pub struct Pin {
    pub name: String,
    pub letter: char,
    pub bit: u8,
    pub pin_addr: u32,
    pub ddr_addr: u32,
    pub port_addr: u32,
}

/// The embedded `std/gpio.ik` source baked into the binary at build time.
fn gpio_src() -> &'static str {
    analysis::STD_FILES
        .iter()
        .find(|(n, _)| *n == "gpio.ik")
        .map(|(_, c)| *c)
        .unwrap_or("")
}

/// Parse a `0x..`-hex or decimal literal as found in a const declaration.
fn parse_addr(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

/// Read the `target <name>` declaration from a source buffer, if present.
pub fn target_of(src: &str) -> Option<String> {
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("target ") {
            let name = rest.split_whitespace().next().unwrap_or("").trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Extract the PIN/DDR/PORT register addresses for every port of `device` from
/// the embedded `std/gpio.ik` table. Ports are returned ordered by letter.
pub fn port_regs(device: &str) -> Vec<PortReg> {
    // letter -> (pin, ddr, port)
    let mut acc: std::collections::BTreeMap<char, (Option<u32>, Option<u32>, Option<u32>)> =
        std::collections::BTreeMap::new();

    let mut in_block = false;
    for line in gpio_src().lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("? target ==") {
            let name = rest.trim().trim_end_matches('{').trim();
            in_block = name == device;
            continue;
        }
        if !in_block {
            continue;
        }
        if t.starts_with('}') {
            // The per-target const blocks contain no nested braces, so the
            // first closing brace ends this device's block.
            break;
        }
        // Expect: const %PINB: u16  = 0x0023
        let rest = match t.strip_prefix("const %") {
            Some(r) => r,
            None => continue,
        };
        let colon = match rest.find(':') {
            Some(i) => i,
            None => continue,
        };
        let reg = rest[..colon].trim(); // e.g. "PINB"
        let value = match rest.find('=') {
            Some(i) => &rest[i + 1..],
            None => continue,
        };
        let addr = match parse_addr(value) {
            Some(a) => a,
            None => continue,
        };

        let (kind, letter) = if let Some(l) = reg.strip_prefix("PORT") {
            (2u8, l)
        } else if let Some(l) = reg.strip_prefix("DDR") {
            (1u8, l)
        } else if let Some(l) = reg.strip_prefix("PIN") {
            (0u8, l)
        } else {
            continue;
        };
        let letter = match letter.chars().next() {
            Some(c) => c,
            None => continue,
        };
        let entry = acc.entry(letter).or_insert((None, None, None));
        match kind {
            0 => entry.0 = Some(addr),
            1 => entry.1 = Some(addr),
            _ => entry.2 = Some(addr),
        }
    }

    acc.into_iter()
        .filter_map(|(letter, (pin, ddr, port))| {
            Some(PortReg {
                letter,
                pin_addr: pin?,
                ddr_addr: ddr?,
                port_addr: port?,
            })
        })
        .collect()
}

/// Expand a device's ports into its individual pins (`P<letter><bit>`), bit 0
/// to 7 of each port. This is the pin list the breadboard offers for wiring.
pub fn pins(device: &str) -> Vec<Pin> {
    let mut out = Vec::new();
    for p in port_regs(device) {
        for bit in 0u8..8 {
            out.push(Pin {
                name: format!("P{}{}", p.letter, bit),
                letter: p.letter,
                bit,
                pin_addr: p.pin_addr,
                ddr_addr: p.ddr_addr,
                port_addr: p.port_addr,
            });
        }
    }
    out
}

/// Data-space addresses of the on-chip serial peripherals' registers, resolved
/// from the simulator's hardware tables. Each is `None` when the device lacks
/// that peripheral. Addresses are data-space (I/O address + 0x20), ready to
/// pass to the VM's `read_data`/`write_data`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PeriphAddrs {
    /// (data register, status register) for USART0.
    pub uart: Option<(u32, u32)>,
    /// (data register, status register) for SPI.
    pub spi: Option<(u32, u32)>,
    /// Data register (TWDR) for TWI/I2C.
    pub twi: Option<u32>,
}

/// Resolve the active target's serial-peripheral register addresses.
pub fn periph_addrs(device: &str) -> PeriphAddrs {
    PeriphAddrs {
        uart: ik8bvm::hw::uart_hw(device).map(|u| (u.data + 0x20, u.status + 0x20)),
        spi: ik8bvm::hw::spi_hw(device).map(|s| (s.data + 0x20, s.status + 0x20)),
        twi: ik8bvm::hw::twi_hw(device).map(|t| t.data + 0x20),
    }
}

/// The full set of data-space addresses the breadboard must watch for a device
/// (every PIN/DDR/PORT register), for the live engine's snapshot list.
pub fn watch_addrs(device: &str) -> Vec<u32> {
    let mut v = Vec::new();
    for p in port_regs(device) {
        v.push(p.pin_addr);
        v.push(p.ddr_addr);
        v.push(p.port_addr);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_atmega328p_ports() {
        let regs = port_regs("atmega328p");
        // The atmega328p exposes ports B, C and D.
        let letters: Vec<char> = regs.iter().map(|r| r.letter).collect();
        assert_eq!(letters, vec!['B', 'C', 'D']);
        let b = regs.iter().find(|r| r.letter == 'B').unwrap();
        assert_eq!(b.pin_addr, 0x23);
        assert_eq!(b.ddr_addr, 0x24);
        assert_eq!(b.port_addr, 0x25);
    }

    #[test]
    fn expands_pins() {
        let pins = pins("atmega328p");
        assert_eq!(pins.len(), 24); // 3 ports * 8 bits
        assert!(pins.iter().any(|p| p.name == "PB5"));
    }

    #[test]
    fn reads_target_decl() {
        assert_eq!(target_of("target atmega328p\n@main {}"), Some("atmega328p".into()));
    }
}
