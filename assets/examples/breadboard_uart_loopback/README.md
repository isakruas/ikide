# UART Loopback

Sends `A`..`Z` over the USART; the loopback device returns every byte to the
receiver, and the LED toggles on each verified round trip.

## On the breadboard

- Add the **UART Loopback** device.
- Add an **LED** wired to **PB5**.

## Run

Run and watch: the LED blinks steadily while bytes round-trip, and the sent
characters appear in the UART tab console.
