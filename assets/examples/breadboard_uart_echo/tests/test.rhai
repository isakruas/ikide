// UART Echo — sends a greeting, then echoes back every received byte verbatim.
//
// Validates the greeting and that an entire input string is echoed, byte-for-byte
// and in order.
build("main.ik");

step_until_uart("Echo ready", 3000000);
check_contains("prints the greeting on start-up", uart_text(), "Echo ready");

uart_clear();
let msg = "Hello, ik!";
uart_send(msg);
step(3000000);
let echoed = uart_recv();
check_contains("echoes the full message verbatim", echoed, msg);
check_eq("echoes exactly the bytes received (no extra/missing)", echoed.len(), msg.len());
