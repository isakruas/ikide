# Timer Interrupt Blinker Example

This example demonstrates how to use Timer1 interrupts to toggle a pin (PB0) at a precise period (approx 524ms) without using blocking CPU loops. It also prints stats over UART when the interrupt fires.

## How to run
1. Start the simulation.
2. Open the **Console** tab on the Serial Monitor.
3. Observe the overflow ticks incrementing periodically while the LED at PB0 toggles.
