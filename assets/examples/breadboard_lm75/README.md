# LM75 Thermostat

Reads the temperature register over I2C four times a second, prints it, and
lights the LED above 30 °C.

## On the breadboard

- Add the **LM75 Temp Sensor 0x48** device.
- Add an **LED** wired to **PB5**.

## Run

Run and drag the device's temperature slider: the UART tab logs `temp:NN`
(also plottable on the Plotter sub-tab) and the LED switches at 30 °C.
