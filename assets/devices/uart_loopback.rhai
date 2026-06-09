// UART loopback: echoes every transmitted byte back to the MCU's receiver.
fn meta() { #{ name: "UART Loopback", bus: "uart" } }
fn init() { #{ q: [] } }

fn uart_tx(byte) { this.q.push(byte); }
fn uart_poll() { if this.q.len() > 0 { this.q.shift() } else { () } }
