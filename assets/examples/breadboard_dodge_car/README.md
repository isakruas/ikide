# Dodge Car 3D on ST7789

A pseudo-3D dodging game, fully simulated: the road converges to a vanishing
point on the horizon, enemy cars rush toward you through 8 depth steps —
growing as they approach — and you slalom between three lanes to survive.

- **U / D** — (menu) pick the difficulty/options; (keyboard) move cursor vertically
- **L / R** — switch lanes, one lane per press; (keyboard) move cursor horizontally
- **SET** — (menu) start the race / select ranking; (game over) back to the menu; (keyboard) select highlighted letter
- **RST** — abandon the race and return to the menu

Difficulties: **EASY** (slow traffic, 1 car), **NORMAL** (faster, 2 cars),
**HARD** (faster still, 2 cars). Every dodged car is a point on the score in
the top-left corner.

If the **AT24C256 I2C EEPROM** is present on the board, a **RANKING** option is unlocked. Reaching a new high score prompts the player to enter their name on an on-screen QWERTY virtual keyboard steered via the Joystick.

The scene is procedural — `@road_id()` in `road.ik` answers "what color lives
at (x, y)?" — so erasing a car is just redrawing the patch of road beneath
it. No frame buffer is needed: 240x240x2 bytes would never fit in 2 KB of
SRAM. The program is split into modules merged by `import`:

- `gfx.ik` — ST7789 SPI driver, rectangle fills and 12x16 text
- `road.ik` — perspective road geometry and the scalable car sprites
- `main.ik` — the game state machine (menu, race, game over, keyboard, leaderboard)

## On the breadboard

- Add the **ST7789 TFT 240x240** device (DC=PB1, CS=PB2).
- Add the **Joystick (5-way + SET/RST)** device (wired as: U=PC0, D=PC1, L=PC2, R=PC3, SET=PD4, RST=PD5 to prevent I2C conflicts).
- Add the **AT24C256 EEPROM** device (PC4=SDA, PC5=SCL) to enable Leaderboards and the Ranking feature.

## Run

Set the Clock to 16 MHz and press Run. The menu draws; choose an option with U/D, press SET and play or view the Leaderboards!
