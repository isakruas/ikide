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

//! A full interactive terminal: a real pseudo-terminal running the user's shell
//! in the workspace directory, with a VT100/ANSI parser maintaining the on-screen
//! grid. Keystrokes are written to the PTY; output is parsed into a [`vt100`]
//! screen the UI renders cell by cell.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

/// One live terminal session: the PTY master, a writer to its input, and the
/// shared parser the reader thread fills from the PTY output.
pub struct Terminal {
    pub title: String,
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    rows: u16,
    cols: u16,
}

/// The shell to launch: `$SHELL`, then common fallbacks per platform.
fn pick_shell() -> String {
    if cfg!(target_os = "windows") {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

impl Terminal {
    /// Launch a shell in `workspace`. `repaint` is invoked whenever new output
    /// arrives so the UI can redraw.
    pub fn spawn<F: Fn() + Send + 'static>(
        workspace: Option<PathBuf>,
        title: String,
        repaint: F,
    ) -> Result<Terminal, String> {
        let rows: u16 = 24;
        let cols: u16 = 80;

        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(pick_shell());
        if let Some(ws) = &workspace {
            cmd.cwd(ws);
        }
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        let killer = child.clone_killer();
        // Drop the slave handle so the child owns the only slave reference and
        // the PTY reports EOF when it exits.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 5000)));
        let parser_reader = parser.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Ok(mut p) = parser_reader.lock() {
                            p.process(&buf[..n]);
                        }
                        repaint();
                    }
                }
            }
        });

        Ok(Terminal { title, parser, writer, master: pair.master, killer, rows, cols })
    }

    /// Send raw bytes (a keystroke translation) to the shell.
    pub fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Resize both the PTY and the parser grid to `rows` x `cols`.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
    }

    /// Shared parser, for the UI to lock and read the current screen.
    pub fn parser(&self) -> Arc<Mutex<vt100::Parser>> {
        self.parser.clone()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}
