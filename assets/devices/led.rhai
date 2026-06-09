// LED: lights when the wired MCU pin drives high.
fn meta() {
    #{
        name: "LED",
        pins: #{ anode: #{} },
        view: [#{ kind: "led", pin: "anode", color: 0xFF5252 }],
    }
}
