// RGB LED: each channel lights from its own MCU pin.
fn meta() {
    #{
        name: "RGB LED",
        pins: #{ r: #{}, g: #{}, b: #{} },
        view: [#{ kind: "rgbled", pins: ["r", "g", "b"] }],
    }
}
