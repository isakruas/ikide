// 8-LED bar graph: one pin per LED.
fn meta() {
    #{
        name: "LED Bar (8)",
        pins: #{ l0: #{}, l1: #{}, l2: #{}, l3: #{}, l4: #{}, l5: #{}, l6: #{}, l7: #{} },
        view: [#{
            kind: "ledbar",
            pins: ["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"],
            color: 0x50FF6E,
        }],
    }
}
