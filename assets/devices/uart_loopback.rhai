// UART loopback: every byte the MCU transmits is sent straight back to its
// receiver. Wire it to the UART bus to test echo programs without a console.
fn meta() { #{ name: "UART Loopback", bus: "uart" } }
fn init()  { #{ q: [] } }

fn uart_tx(byte) { this.q.push(byte); }
fn uart_poll()   { if this.q.len() > 0 { this.q.shift() } else { () } }
