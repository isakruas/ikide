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

//! Host side of the on-board ik serial-bootloader protocol.
//!
//! Frame  host -> target:  `0x1B 'i' 'k'  CMD  payload  CRC8`
//! Reply  target -> host:  `ACK (0x06)` or `NAK (0x15)`, then command-specific bytes.
//! The CRC is CRC-8/MAXIM (poly 0x8C reflected, init 0) over the payload only
//! (not CMD), the same algorithm the device's `std/crc` `@crc8` uses.
//!
//! There is no HELLO handshake and no length field: each command's payload
//! length is fixed (RUN: none; WRITE: 2 address bytes + one flash page), so the
//! device reads exactly the right number of bytes from the command alone. The
//! host derives the page size and boot-section start from the selected MCU
//! (`bootloader_params`) instead of querying the device, and detects a live
//! loader by retrying the first WRITE across the board-reset window.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const SOF: [u8; 3] = [0x1B, b'i', b'k'];
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const CMD_WRITE: u8 = 0x02;
const CMD_RUN: u8 = 0x04;

/// CRC-8/MAXIM (Dallas/Maxim) — must match `std/crc`'s `@crc8` on the device.
fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            let odd = crc & 1;
            crc >>= 1;
            if odd != 0 {
                crc ^= 0x8C;
            }
        }
    }
    crc
}

/// Build a complete on-wire frame: sync prefix + CMD + payload + CRC8.
/// The CRC covers the payload only, matching the device.
fn frame(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(SOF.len() + 1 + payload.len() + 1);
    f.extend_from_slice(&SOF);
    f.push(cmd);
    f.extend_from_slice(payload);
    f.push(crc8(payload));
    f
}

/// Flash page size and boot-section start for a supported MCU, mirroring the
/// per-target blocks in `std/bootloader.ik`. `None` if unsupported.
fn bootloader_params(device: &str) -> Option<(usize, usize)> {
    let d = device.trim().to_lowercase();
    if !has_bootloader_support(&d) {
        return None;
    }
    // 64 KB parts use 256-byte pages; the 32 KB mega parts use 128-byte pages.
    // at90can32 is the only 32 KB part with 256-byte pages and a 0x6000 boot.
    let params = match d.as_str() {
        "at90can32" => (256, 0x6000),
        "at90can64" => (256, 0xE000),
        "atmega64" | "atmega64a" | "atmega640" | "atmega644" | "atmega644a" | "atmega644p" | "atmega644pa" | "atmega645" | "atmega645a" | "atmega645p" | "atmega6450" | "atmega6450a" | "atmega6450p" | "atmega649" | "atmega649a" | "atmega649p" | "atmega6490" | "atmega6490a" | "atmega6490p" => (256, 0xE000),
        "atmega32" | "atmega32a" => (128, 0x7800),
        _ => (128, 0x7000),
    };
    Some(params)
}

/// Decode an Intel HEX image into a flat byte vector starting at address 0.
fn parse_ihex(text: &str) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut base: u32 = 0;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec = line.strip_prefix(':').ok_or_else(|| format!("line {}: not an Intel HEX record", n + 1))?;
        let bytes = decode_hex(rec).map_err(|e| format!("line {}: {}", n + 1, e))?;
        if bytes.len() < 5 {
            return Err(format!("line {}: record too short", n + 1));
        }
        let len = bytes[0] as usize;
        if bytes.len() != 5 + len {
            return Err(format!("line {}: length mismatch", n + 1));
        }
        if bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b)) != 0 {
            return Err(format!("line {}: bad checksum", n + 1));
        }
        let addr = ((bytes[1] as u32) << 8) | bytes[2] as u32;
        match bytes[3] {
            0x00 => {
                let start = (base + addr) as usize;
                let end = start + len;
                if out.len() < end {
                    out.resize(end, 0);
                }
                out[start..end].copy_from_slice(&bytes[4..4 + len]);
            }
            0x01 => break,
            0x02 => base = (((bytes[4] as u32) << 8) | bytes[5] as u32) << 4,
            0x04 => base = (((bytes[4] as u32) << 8) | bytes[5] as u32) << 16,
            _ => {}
        }
    }
    Ok(out)
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "invalid hex digit".to_string()))
        .collect()
}

type Port = Box<dyn serialport::SerialPort>;

/// Read one byte, retrying until `deadline`. `None` on timeout.
fn read_byte(port: &mut Port, deadline: Instant) -> Option<u8> {
    let mut buf = [0u8; 1];
    while Instant::now() < deadline {
        match port.read(&mut buf) {
            Ok(1) => return Some(buf[0]),
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return None,
        }
    }
    None
}

/// Send a frame and wait for a single ACK/NAK byte. `Ok(true)` = ACK.
fn exchange(port: &mut Port, f: &[u8], reply_timeout: Duration) -> Result<bool, String> {
    port.write_all(f).map_err(|e| format!("serial write failed: {}", e))?;
    let _ = port.flush();
    match read_byte(port, Instant::now() + reply_timeout) {
        Some(ACK) => Ok(true),
        Some(NAK) => Ok(false),
        Some(other) => Err(format!("unexpected reply 0x{:02X}", other)),
        None => Err("no reply (timeout)".into()),
    }
}

/// Build the WRITE frame for one page at `addr` (`page` already padded).
fn write_frame(addr: u16, page: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + page.len());
    payload.push((addr >> 8) as u8);
    payload.push((addr & 0xFF) as u8);
    payload.extend_from_slice(page);
    frame(CMD_WRITE, &payload)
}

/// Write the first page, retrying across `window` to cover the user resetting
/// the board. Any ACK proves the loader is live; this replaces the old HELLO
/// probe — the first page is real work, not a throwaway handshake.
fn write_first_page(port: &mut Port, f: &[u8], window: Duration, log: &dyn Fn(String)) -> Result<(), String> {
    log("Waiting for the bootloader — reset the board if nothing happens…".into());
    let deadline = Instant::now() + window;
    loop {
        // Drop stale input so a reply buffered during reset can't desync us.
        let _ = port.clear(serialport::ClearBuffer::Input);
        if port.write_all(f).is_ok() {
            let _ = port.flush();
            match read_byte(port, Instant::now() + Duration::from_millis(400)) {
                Some(ACK) => return Ok(()),
                Some(NAK) => {} // alive but rejected (CRC/garbled) — resend
                Some(other) => return Err(format!("unexpected reply 0x{:02X}", other)),
                None => {}
            }
        }
        if Instant::now() >= deadline {
            return Err("no response from the bootloader (is it running, baud correct, board reset?)".into());
        }
    }
}

/// Write one page with retries (used after the board is known to be live).
fn write_page(port: &mut Port, addr: u16, f: &[u8], log: &dyn Fn(String)) -> Result<(), String> {
    for attempt in 1..=5 {
        match exchange(port, f, Duration::from_millis(800)) {
            Ok(true) => return Ok(()),
            Ok(false) => log(format!("  page @0x{:04X}: device NAK (attempt {}/5)", addr, attempt)),
            Err(e) => log(format!("  page @0x{:04X}: {} (attempt {}/5)", addr, e, attempt)),
        }
    }
    Err(format!("page @0x{:04X} failed after 5 attempts", addr))
}

/// Drive the whole upload. `device` selects the page size and boot-section start
/// (there is no on-wire HELLO). `log` receives human-readable progress lines.
pub fn upload(device: &str, port_name: &str, baud: u32, hex_path: &Path, log: &dyn Fn(String)) -> Result<(), String> {
    let (page_size, app_end) = bootloader_params(device)
        .ok_or_else(|| format!("{} is not supported by the serial bootloader", device))?;

    let text = fs::read_to_string(hex_path).map_err(|e| format!("cannot read {:?}: {}", hex_path, e))?;
    let image = parse_ihex(&text)?;
    if image.is_empty() {
        return Err("the compiled image is empty".into());
    }
    if image.len() > app_end {
        return Err(format!(
            "image is {} B but the application section is only {} B — too large for this bootloader",
            image.len(),
            app_end
        ));
    }

    let mut port: Port = serialport::new(port_name, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|e| format!("cannot open {} @ {} baud: {}", port_name, baud, e))?;

    log(format!(
        "Bootloader target {} — page {} B, application section ends at 0x{:04X}.",
        device, page_size, app_end
    ));

    let page = page_size.max(2);
    let pages = image.len().div_ceil(page);
    log(format!("Programming {} B in {} page(s)…", image.len(), pages));
    for p in 0..pages {
        let addr = p * page;
        let mut chunk = vec![0xFFu8; page]; // pad the last page with the erased value
        let n = (image.len() - addr).min(page);
        chunk[..n].copy_from_slice(&image[addr..addr + n]);
        let f = write_frame(addr as u16, &chunk);
        // The first page doubles as the liveness probe: retry it across the
        // reset window. Once the board ACKs it, the rest use the normal retry.
        if p == 0 {
            write_first_page(&mut port, &f, Duration::from_secs(8), log)?;
        } else {
            write_page(&mut port, addr as u16, &f, log)?;
        }
        log(format!("  page {}/{} @ 0x{:04X} ok", p + 1, pages, addr));
    }

    log("Starting the application…".into());
    match exchange(&mut port, &frame(CMD_RUN, &[]), Duration::from_millis(800)) {
        Ok(true) => {
            log("Upload complete — application running.".into());
            Ok(())
        }
        Ok(false) => Err("device NAKed the run command".into()),
        // The device may jump to the app before its ACK is read; treat a missing
        // reply after a fully-written image as success.
        Err(_) => {
            log("Upload complete (device started the application).".into());
            Ok(())
        }
    }
}

/// Returns true if the device is supported by `std/bootloader`.
///
/// This list mirrors the per-target blocks in `std/bootloader.ik`. Only 32 KB
/// and 64 KB parts are included: the bootloader is ~3.3 KB, which is ≤11% of
/// flash on those parts but 20–40% on 16 KB/8 KB parts. Parts whose name
/// avrdude does not recognise (the `ata*` family) are also excluded.
pub fn has_bootloader_support(device: &str) -> bool {
    let d = device.trim().to_lowercase();
    matches!(
        d.as_str(),
        // 32 KB
        "at90can32" |
        "atmega32" | "atmega32a" |
        "atmega324a" | "atmega324p" | "atmega324pa" |
        "atmega325" | "atmega325a" | "atmega325p" | "atmega325pa" |
        "atmega3250" | "atmega3250a" | "atmega3250p" | "atmega3250pa" |
        "atmega328" | "atmega328p" |
        "atmega329" | "atmega329a" | "atmega329p" | "atmega329pa" |
        "atmega3290" | "atmega3290a" | "atmega3290p" | "atmega3290pa" |
        // 64 KB
        "at90can64" |
        "atmega64" | "atmega64a" | "atmega640" |
        "atmega644" | "atmega644a" | "atmega644p" | "atmega644pa" |
        "atmega645" | "atmega645a" | "atmega645p" |
        "atmega6450" | "atmega6450a" | "atmega6450p" |
        "atmega649" | "atmega649a" | "atmega649p" |
        "atmega6490" | "atmega6490a" | "atmega6490p"
    )
}

/// Suggest fuse flags for burning the bootloader on the given device.
pub fn suggest_burn_fuse_flags(device: &str) -> String {
    let d = device.trim().to_lowercase();
    if d.is_empty() {
        String::new()
    } else if d == "atmega328p" || d == "atmega328" {
        "-U lfuse:w:0xFF:m -U hfuse:w:0xD8:m -U efuse:w:0xFD:m".to_string()
    } else if d == "atmega32" || d == "atmega32a" {
        "-U lfuse:w:0xFF:m -U hfuse:w:0xD8:m".to_string()
    } else {
        "-U lfuse:w:0xFF:m -U hfuse:w:0xD8:m".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_matches_std_maxim() {
        // CRC-8/MAXIM check value for "123456789" is 0xA1.
        assert_eq!(crc8(b"123456789"), 0xA1);
    }

    #[test]
    fn parses_a_basic_ihex() {
        // Two data bytes 0xAB,0xCD at 0x0000, then EOF.
        let hex = ":02000000ABCD86\n:00000001FF\n";
        let img = parse_ihex(hex).unwrap();
        assert_eq!(img, vec![0xAB, 0xCD]);
    }

    #[test]
    fn frame_has_sync_and_payload_crc() {
        // RUN: empty payload, so the CRC over nothing is the init value 0.
        let f = frame(CMD_RUN, &[]);
        assert_eq!(&f[0..3], &SOF);
        assert_eq!(f[3], CMD_RUN);
        assert_eq!(f[4], 0x00);
        assert_eq!(f.len(), 5);

        // WRITE: CRC covers the payload only, not CMD.
        let payload = [0x12u8, 0x34, 0xAB, 0xCD];
        let w = frame(CMD_WRITE, &payload);
        assert_eq!(w[3], CMD_WRITE);
        assert_eq!(&w[4..8], &payload);
        assert_eq!(w[8], crc8(&payload));
    }

    #[test]
    fn params_track_supported_parts() {
        assert_eq!(bootloader_params("atmega328p"), Some((128, 0x7000)));
        assert_eq!(bootloader_params("atmega640"), Some((256, 0xE000)));
        assert_eq!(bootloader_params("at90can32"), Some((256, 0x6000)));
        assert_eq!(bootloader_params("attiny85"), None);
    }
}
