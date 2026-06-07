# IK8B Examples & Bootloader Guide

This folder contains a serial bootloader and a simple blink example for the ATmega32 written in IK.

## 1. Burning the Bootloader (First Time Setup)

To use the serial bootloader, you must first burn `bootloader.hex` to your ATmega32 using a hardware ISP programmer (like a USBasp). You also **must** configure the microcontroller fuses correctly so that the hardware always runs the bootloader before your application.

### Required Fuses for ATmega32 (8MHz External Crystal)
- **lfuse**: `0xFF` (External Crystal 8MHz+)
- **hfuse**: `0xD8` (BOOTRST enabled, BOOTSZ=2048W, JTAG disabled)

### Flashing via AVRDUDE
If you are using a standard USBasp programmer, you can flash the bootloader and set the fuses in a single command:

```bash
sudo avrdude -c usbasp -p m32 -B 10 -U lfuse:w:0xFF:m -U hfuse:w:0xD8:m -U flash:w:build/bootloader.hex:i
```

---

## 2. Hardware UART Wiring

Once the bootloader is inside the chip, you can program the ATmega32 using a simple USB-to-Serial adapter (like FTDI, CH340, CP2102).

**Wiring is CROSSED:**
- **Adapter RX** ---> **MCU TX (Pin PD1)**
- **Adapter TX** ---> **MCU RX (Pin PD0)**
- **Adapter GND** ---> **MCU GND**

---

## 3. Uploading Applications (Serial Upload)

Now that the bootloader is running, uploading applications like `blink_pb0.hex` is fast and easy.

1. Open your IK IDE.
2. Ensure the correct Serial Port is selected (e.g., `/dev/ttyUSB0` or `COM3`).
3. Click to upload `blink_pb0.hex`.
4. The IDE will display: `Waiting for the bootloader — reset the board if nothing happens…`
5. **Press the physical Reset button on your board.**
6. The bootloader will instantly catch the signal, upload your application, and execute it!

*(Note: The bootloader waits for about 5 seconds after a reset. If no upload signal is received in that time, it automatically jumps to address `0x0000` and runs your previously installed application).*
