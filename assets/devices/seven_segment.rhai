// 7-segment digit (common cathode): segments a..g + dp, one pin each,
// segment lit when its pin drives high.
fn meta() {
    #{
        name: "7-Segment",
        pins: #{ a: #{}, b: #{}, c: #{}, d: #{}, e: #{}, f: #{}, g: #{}, dp: #{} },
        view: [#{ kind: "sevenseg", pins: ["a", "b", "c", "d", "e", "f", "g", "dp"] }],
    }
}
