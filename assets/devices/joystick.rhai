// 5-way navigation joystick with SET and RST buttons (U/D/L/R + center
// actions). Each output is an independent active-low line: it idles high and
// is pulled low while held — enable the MCU's internal pull-ups and read 0
// as "pressed". Defaults wire the six lines to PC0..PC5; rewire on the card.
fn meta() {
    #{
        name: "Joystick (5-way + SET/RST)",
        pins: #{
            u:   #{ mode: "drive", idle: 1, "default": "PC0" },
            d:   #{ mode: "drive", idle: 1, "default": "PC1" },
            l:   #{ mode: "drive", idle: 1, "default": "PC2" },
            r:   #{ mode: "drive", idle: 1, "default": "PC3" },
            set: #{ mode: "drive", idle: 1, "default": "PC4" },
            rst: #{ mode: "drive", idle: 1, "default": "PC5" },
        },
        view: [
            #{ kind: "button", pin: "u", press: 0, label: "U" },
            #{ kind: "button", pin: "l", press: 0, label: "L" },
            #{ kind: "button", pin: "r", press: 0, label: "R" },
            #{ kind: "button", pin: "d", press: 0, label: "D" },
            #{ kind: "button", pin: "set", press: 0, label: "SET" },
            #{ kind: "button", pin: "rst", press: 0, label: "RST" },
        ],
    }
}
