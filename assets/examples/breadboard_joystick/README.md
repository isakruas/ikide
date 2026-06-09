# Joystick Cursor

A 5-way navigation joystick drives a cursor on an LED bar. The six lines are
active-low with the MCU pull-ups enabled: pressed reads 0, and presses are
edge-detected so each tap acts once.

- **L / R** step the lit position across the bar
- **RST** recenters the cursor
- **SET** fills the whole bar while held
- every press (U, D, L, R, RST) is logged over UART

## On the breadboard

- Add the **Joystick (5-way + SET/RST)** device — its terminals default to
  **PC0..PC5** (u, d, l, r, set, rst).
- Add an **LED Bar (8)** wired **l0..l7 = PD0..PD7**.

## Run

Set the Clock to 16 MHz, press Run, and use the joystick buttons on the
device card: the cursor walks with L/R, SET floods the bar, and the UART tab
logs each press.
