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
//! Frame  host -> target:  `0x1B 'i' 'k'  CMD  LEN_HI LEN_LO  payload  CRC_HI CRC_LO`
//! Reply  target -> host:  `ACK (0x06)` or `NAK (0x15)`, then command-specific bytes.
//! The CRC is CRC-16/ARC (poly 0xA001, init 0) over `CMD,LEN_HI,LEN_LO,payload`,
//! the same algorithm the device's `std/crc` `@crc16` uses.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const SOF: [u8; 3] = [0x1B, b'i', b'k'];
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const CMD_HELLO: u8 = 0x01;
const CMD_WRITE: u8 = 0x02;
const CMD_RUN: u8 = 0x04;

/// CRC-16/ARC — must match `std/crc`'s `@crc16` on the device.
fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            let odd = crc & 1;
            crc >>= 1;
            if odd != 0 {
                crc ^= 0xA001;
            }
        }
    }
    crc
}

/// Build a complete on-wire frame (sync prefix + body + CRC).
fn frame(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(3 + payload.len());
    body.push(cmd);
    body.push((payload.len() >> 8) as u8);
    body.push((payload.len() & 0xFF) as u8);
    body.extend_from_slice(payload);
    let crc = crc16(&body);
    let mut f = Vec::with_capacity(3 + body.len() + 2);
    f.extend_from_slice(&SOF);
    f.extend_from_slice(&body);
    f.push((crc >> 8) as u8);
    f.push((crc & 0xFF) as u8);
    f
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

/// Device parameters returned by HELLO.
struct Hello {
    version: u8,
    page_size: usize,
    app_end: usize,
}

/// Handshake, retrying for `window` to cover the user resetting the board.
fn hello(port: &mut Port, window: Duration, log: &dyn Fn(String)) -> Result<Hello, String> {
    let f = frame(CMD_HELLO, &[]);
    let deadline = Instant::now() + window;
    log("Waiting for the bootloader — reset the board if nothing happens…".into());
    while Instant::now() < deadline {
        // Discard anything stale before each probe so a buffered reply from an
        // earlier HELLO can't desync the rest of the session.
        let _ = port.clear(serialport::ClearBuffer::Input);
        if port.write_all(&f).is_ok() {
            let _ = port.flush();
            if read_byte(port, Instant::now() + Duration::from_millis(300)) == Some(ACK) {
                let info_deadline = Instant::now() + Duration::from_millis(500);
                let mut info = [0u8; 5];
                let mut got = 0;
                while got < 5 {
                    match read_byte(port, info_deadline) {
                        Some(b) => {
                            info[got] = b;
                            got += 1;
                        }
                        None => break,
                    }
                }
                if got == 5 {
                    // Drain any extra ACKs from HELLOs the device buffered while
                    // it was starting up, so WRITE starts on a clean stream.
                    std::thread::sleep(Duration::from_millis(50));
                    let _ = port.clear(serialport::ClearBuffer::Input);
                    return Ok(Hello {
                        version: info[0],
                        page_size: ((info[1] as usize) << 8) | info[2] as usize,
                        app_end: ((info[3] as usize) << 8) | info[4] as usize,
                    });
                }
            }
        }
    }
    Err("no response from the bootloader (is it running, baud correct, board reset?)".into())
}

/// Write one page with retries.
fn write_page(port: &mut Port, addr: u16, page: &[u8], log: &dyn Fn(String)) -> Result<(), String> {
    let mut payload = Vec::with_capacity(2 + page.len());
    payload.push((addr >> 8) as u8);
    payload.push((addr & 0xFF) as u8);
    payload.extend_from_slice(page);
    let f = frame(CMD_WRITE, &payload);
    for attempt in 1..=5 {
        match exchange(port, &f, Duration::from_millis(800)) {
            Ok(true) => return Ok(()),
            Ok(false) => log(format!("  page @0x{:04X}: device NAK (attempt {}/5)", addr, attempt)),
            Err(e) => log(format!("  page @0x{:04X}: {} (attempt {}/5)", addr, e, attempt)),
        }
    }
    Err(format!("page @0x{:04X} failed after 5 attempts", addr))
}

/// Drive the whole upload. `log` receives human-readable progress lines.
pub fn upload(port_name: &str, baud: u32, hex_path: &Path, log: &dyn Fn(String)) -> Result<(), String> {
    let text = fs::read_to_string(hex_path).map_err(|e| format!("cannot read {:?}: {}", hex_path, e))?;
    let image = parse_ihex(&text)?;
    if image.is_empty() {
        return Err("the compiled image is empty".into());
    }

    let mut port: Port = serialport::new(port_name, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|e| format!("cannot open {} @ {} baud: {}", port_name, baud, e))?;

    let info = hello(&mut port, Duration::from_secs(8), log)?;
    log(format!(
        "Bootloader v{} — page {} B, application section ends at 0x{:04X}.",
        info.version, info.page_size, info.app_end
    ));
    if image.len() > info.app_end {
        return Err(format!(
            "image is {} B but the application section is only {} B — too large for this bootloader",
            image.len(),
            info.app_end
        ));
    }

    let page = info.page_size.max(2);
    let pages = image.len().div_ceil(page);
    log(format!("Programming {} B in {} page(s)…", image.len(), pages));
    for p in 0..pages {
        let addr = p * page;
        let mut chunk = vec![0xFFu8; page]; // pad the last page with the erased value
        let n = (image.len() - addr).min(page);
        chunk[..n].copy_from_slice(&image[addr..addr + n]);
        write_page(&mut port, addr as u16, &chunk, log)?;
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
pub fn has_bootloader_support(device: &str) -> bool {
    let d = device.trim().to_lowercase();
    matches!(
        d.as_str(),
        "at90can32" | "at90can64" |
        "at90pwm216" | "at90pwm2b" | "at90pwm316" | "at90pwm3b" |
        "ata6612c" | "ata6613c" | "ata6614q" |
        "atmega16" | "atmega162" | "atmega163" | "atmega164a" | "atmega164p" | "atmega164pa" |
        "atmega165" | "atmega165a" | "atmega165p" | "atmega165pa" |
        "atmega168" | "atmega168a" | "atmega168p" | "atmega168pa" |
        "atmega169" | "atmega169a" | "atmega169p" | "atmega169pa" |
        "atmega16a" |
        "atmega32" | "atmega324a" | "atmega324p" | "atmega324pa" |
        "atmega325" | "atmega3250" | "atmega3250a" | "atmega3250p" | "atmega3250pa" |
        "atmega325a" | "atmega325p" | "atmega325pa" |
        "atmega328" | "atmega328p" |
        "atmega329" | "atmega3290" | "atmega3290a" | "atmega3290p" | "atmega3290pa" |
        "atmega329a" | "atmega329p" | "atmega329pa" |
        "atmega32a" | "atmega32c1" | "atmega32hvb" | "atmega32hvbrevb" | "atmega32m1" |
        "atmega32u2" | "atmega32u4" | "atmega32u6" |
        "atmega64" | "atmega640" | "atmega644" | "atmega644a" | "atmega644p" |
        "atmega644pa" | "atmega645" | "atmega6450" | "atmega6450a" | "atmega6450p" |
        "atmega645a" | "atmega649" | "atmega6490" | "atmega6490a" | "atmega6490p" |
        "atmega649a" | "atmega64a" | "atmega64c1" | "atmega64m1" |
        "atmega8" | "atmega8515" | "atmega8535" | "atmega88" | "atmega88a" |
        "atmega88p" | "atmega88pa" | "atmega8a" | "atmega8hva" | "atmega8u2"
    )
}

/// Suggest fuse flags for burning the bootloader on the given device.
pub fn suggest_burn_fuse_flags(device: &str) -> String {
    let d = device.trim().to_lowercase();
    if d.is_empty() {
        String::new()
    } else if d == "atmega328p" || d == "atmega328" {
        "-U lfuse:w:0xFF:m -U hfuse:w:0xD8:m -U efuse:w:0xFD:m".to_string()
    } else if d == "atmega32" || d == "atmega32a" || d == "atmega16" || d == "atmega16a" {
        "-U lfuse:w:0xFF:m -U hfuse:w:0xD8:m".to_string()
    } else {
        "-U lfuse:w:0xFF:m -U hfuse:w:0xD8:m".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_matches_std_arc() {
        // CRC-16/ARC check value for "123456789" is 0xBB3D.
        assert_eq!(crc16(b"123456789"), 0xBB3D);
    }

    #[test]
    fn parses_a_basic_ihex() {
        // Two data bytes 0xAB,0xCD at 0x0000, then EOF.
        let hex = ":02000000ABCD86\n:00000001FF\n";
        let img = parse_ihex(hex).unwrap();
        assert_eq!(img, vec![0xAB, 0xCD]);
    }

    #[test]
    fn frame_has_sync_and_crc() {
        let f = frame(CMD_HELLO, &[]);
        assert_eq!(&f[0..3], &SOF);
        assert_eq!(f[3], CMD_HELLO);
        assert_eq!(&f[4..6], &[0x00, 0x00]); // len = 0
        let crc = crc16(&[CMD_HELLO, 0x00, 0x00]);
        assert_eq!(f[6], (crc >> 8) as u8);
        assert_eq!(f[7], (crc & 0xFF) as u8);
    }
}
