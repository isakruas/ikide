// Potentiometer: a slider feeding an ADC channel (0..1023).
fn meta() {
    #{
        name: "Potentiometer",
        pins: #{ wiper: #{ mode: "adc" } },
        view: [#{ kind: "slider", pin: "wiper", min: 0, max: 1023 }],
    }
}
