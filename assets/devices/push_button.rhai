// Push button to GND: pulls the wired pin low while held.
// Enable the MCU's internal pull-up; the released level is high.
fn meta() {
    #{
        name: "Push Button",
        pins: #{ pin: #{ mode: "drive", idle: 1 } },
        view: [#{ kind: "button", pin: "pin", press: 0, label: "hold" }],
    }
}
