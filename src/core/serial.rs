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

// Serial monitor backend.
//
// A reader thread polls the open port and streams whatever the board sends to
// the UI over a channel; the writer half stays on the UI thread for sending.
// Closing the connection (or dropping it) signals the thread to stop.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A message from the reader thread to the UI.
pub enum SerialMsg {
    Data(String),
    Error(String),
    Closed,
}

/// An open serial connection: a writer kept on the UI side plus a background
/// reader feeding `rx`.
pub struct SerialConn {
    writer: Box<dyn serialport::SerialPort>,
    stop: Arc<AtomicBool>,
    pub rx: Receiver<SerialMsg>,
    pub port: String,
    pub baud: u32,
}

impl SerialConn {
    /// Open `port` at `baud` and spawn the reader thread.
    pub fn open(port: &str, baud: u32) -> Result<Self, String> {
        let reader = serialport::new(port, baud)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| e.to_string())?;
        // A clone gives an independent handle the UI thread can write through
        // while the reader thread owns the original for reading.
        let writer = reader.try_clone().map_err(|e| e.to_string())?;

        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        {
            let stop = stop.clone();
            let mut reader = reader;
            thread::spawn(move || {
                let mut buf = [0u8; 2048];
                while !stop.load(Ordering::Relaxed) {
                    match reader.read(&mut buf) {
                        Ok(0) => {}
                        Ok(n) => {
                            let s = String::from_utf8_lossy(&buf[..n]).into_owned();
                            if tx.send(SerialMsg::Data(s)).is_err() {
                                break;
                            }
                        }
                        // A read timeout is normal: it lets us re-check `stop`.
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) => {
                            let _ = tx.send(SerialMsg::Error(e.to_string()));
                            break;
                        }
                    }
                }
                let _ = tx.send(SerialMsg::Closed);
            });
        }

        Ok(Self {
            writer,
            stop,
            rx,
            port: port.to_string(),
            baud,
        })
    }

    /// Write raw bytes to the port.
    pub fn send(&mut self, data: &[u8]) -> Result<(), String> {
        self.writer.write_all(data).map_err(|e| e.to_string())
    }
}

impl Drop for SerialConn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Best-effort list of serial ports the system exposes.
pub fn list_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

/// Common baud rates offered in the UI.
pub const BAUD_RATES: &[u32] = &[300, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400];

/// The line ending appended to a sent line.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    None,
    Lf,
    Cr,
    CrLf,
}

impl LineEnding {
    pub fn suffix(self) -> &'static str {
        match self {
            LineEnding::None => "",
            LineEnding::Lf => "\n",
            LineEnding::Cr => "\r",
            LineEnding::CrLf => "\r\n",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineEnding::None => "No line ending",
            LineEnding::Lf => "Newline (LF)",
            LineEnding::Cr => "Carriage return (CR)",
            LineEnding::CrLf => "Both (CRLF)",
        }
    }
}
