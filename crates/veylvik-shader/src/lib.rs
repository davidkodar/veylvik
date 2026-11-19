//! Custom shader nodes: the user writes one WGSL function, `shade`, and declares its controls in comments. This crate checks the text (bounded size, no bindings or entry points of its own, loops only of a fixed small length) and builds the full compute program around it. No GPU here.
use std::fmt::Write;

/// Longest accepted source, in bytes.
pub const MAX_SOURCE: usize = 16_000;
/// Control values a shader may use (a slider takes one, a colour three).
pub const SLOTS: usize = 16;
/// Most iterations of one loop, and of nested loops multiplied together.
pub const MAX_LOOP: i64 = 1024;
/// Most `for` loops in a shader.
pub const MAX_LOOPS: usize = 8;
/// The code a new shader node starts with.
pub const EXAMPLE: &str = "// slider amount 0 1 0.5\n// colour tint 1 0.8 0.6\n\n// `at` is the pixel's centre. image(at) reads the input there.\nfn shade(at: vec2<f32>) -> vec4<f32> {\n    let c = image(at);\n    return vec4<f32>(mix(c.rgb, c.rgb * u.tint, u.amount), c.a);\n}\n";
/// The slot values `EXAMPLE` declares, for the node's defaults.
pub const EXAMPLE_DEFAULTS: [f32; SLOTS] = [
    0.5, 1.0, 0.8, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

/// A control declared in the shader's comments.
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    /// `// slider NAME MIN MAX DEFAULT`: one number, `u.NAME`.
    Slider {
        name: String,
        slot: usize,
        min: f32,
        max: f32,
        default: f32,
    },
    /// `// colour NAME R G B`: linear RGB in three slots, `u.NAME` (a vec3).
    Colour {
        name: String,
        slot: usize,
        default: [f32; 3],
    },
}
impl Control {
    pub fn name(&self) -> &str {
        match self {
            Self::Slider { name, .. } | Self::Colour { name, .. } => name,
        }
    }
}
fn fail<T>(message: impl Into<String>) -> Result<T, String> {
    Err(message.into())
}
fn identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && word.len() <= 32
}
/// The controls declared in `code`, in order, with their slots.
pub fn controls(code: &str) -> Result<Vec<Control>, String> {
    let mut out: Vec<Control> = Vec::new();
    let mut slot = 0;
    for (number, line) in code.lines().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("//") else {
            continue;
        };
        let words: Vec<&str> = rest.split_whitespace().collect();
        let kind = match words.first() {
            Some(&"slider") => 1,
            Some(&"colour" | &"color") => 3,
            _ => continue,
        };
        let line = number + 1;
        let numbers: Vec<f32> = words
            .iter()
            .skip(2)
            .map(|w| w.parse::<f32>().ok().filter(|v| v.is_finite()))
            .collect::<Option<_>>()
            .ok_or(format!(
                "line {line}: the values after the name must be numbers"
            ))?;
        let name = words.get(1).copied().unwrap_or_default();
        if !identifier(name) {
            return fail(format!(
                "line {line}: a control needs a name of letters, digits and _ (at most 32), starting with a letter"
            ));
        }
        if out.iter().any(|c| c.name() == name) {
            return fail(format!("line {line}: two controls are called {name}"));
        }
        if slot + kind > SLOTS {
            return fail(format!(
                "line {line}: a shader has {SLOTS} control values; a slider uses one and a colour three"
            ));
        }
        out.push(if kind == 1 {
            let [min, max, default] = numbers[..] else {
                return fail(format!("line {line}: write // slider NAME MIN MAX DEFAULT"));
            };
            if !(min < max && (min..=max).contains(&default)) || min < -1.0e6 || max > 1.0e6 {
                return fail(format!(
                    "line {line}: a slider needs MIN below MAX, DEFAULT between them, within ±1000000"
                ));
            }
            Control::Slider {
                name: name.into(),
                slot,
                min,
                max,
                default,
            }
        } else {
            let [r, g, b] = numbers[..] else {
                return fail(format!("line {line}: write // colour NAME RED GREEN BLUE"));
            };
            if [r, g, b].iter().any(|v| !(0.0..=64.0).contains(v)) {
                return fail(format!("line {line}: colour values are 0 to 64 (linear)"));
            }
            Control::Colour {
                name: name.into(),
                slot,
                default: [r, g, b],
            }
        });
        slot += kind;
    }
    Ok(out)
}
/// The slot values the declarations ask for by default.
pub fn defaults(controls: &[Control]) -> [f32; SLOTS] {
    let mut out = [0.0; SLOTS];
    for control in controls {
        match control {
            Control::Slider { slot, default, .. } => out[*slot] = *default,
            Control::Colour { slot, default, .. } => out[*slot..*slot + 3].copy_from_slice(default),
        }
    }
    out
}
/// `code` with comments blanked out (same length and line breaks).
fn without_comments(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < bytes.len() {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(if bytes[i] == b'\n' { '\n' } else { ' ' });
                    i += 1;
                }
            }
        } else {
            // Non-ASCII outside comments is rejected by `check`; keep lengths equal.
            out.push(if bytes[i].is_ascii() {
                bytes[i] as char
            } else {
                '?'
            });
            i += 1;
        }
    }
    out
}
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}
/// Byte offsets of whole-word occurrences of `word`.
fn words<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let bytes = text.as_bytes();
    let part = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    text.match_indices(word).map(|(i, _)| i).filter(move |&i| {
        (i == 0 || !part(bytes[i - 1])) && bytes.get(i + word.len()).is_none_or(|b| !part(*b))
    })
}
/// The offset of the bracket closing the one at `open`.
fn closing(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let (a, b) = match bytes[open] {
        b'(' => (b'(', b')'),
        _ => (b'{', b'}'),
    };
    let mut depth = 0;
    for (i, c) in bytes.iter().enumerate().skip(open) {
        if *c == a {
            depth += 1;
        } else if *c == b {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}
/// One `for` loop: where its body is, its counter and how often it runs.
struct Loop {
    body: std::ops::Range<usize>,
    count: i64,
}
fn parse_loop(text: &str, at: usize) -> Result<Loop, String> {
    let line = line_of(text, at);
    let form = || {
        format!(
            "line {line}: write loops as for (var i = 0; i < 16; i++), with whole numbers (so their length is known)"
        )
    };
    let open = at + text[at..].find('(').ok_or_else(form)?;
    if !text[at + 3..open].trim().is_empty() {
        return Err(form());
    }
    let close = closing(text, open).ok_or_else(form)?;
    let parts: Vec<&str> = text[open + 1..close].split(';').map(str::trim).collect();
    let [init, test, step] = parts[..] else {
        return Err(form());
    };
    let number = |s: &str| {
        s.trim()
            .trim_end_matches(['i', 'u'])
            .parse::<i64>()
            .ok()
            .filter(|v| v.abs() <= 1_000_000)
    };
    // var NAME [: TYPE] = START
    let init = init.strip_prefix("var").ok_or_else(form)?;
    let (declared, start) = init.split_once('=').ok_or_else(form)?;
    let name = declared.split(':').next().unwrap_or_default().trim();
    let start = number(start).ok_or_else(form)?;
    if !identifier(name) {
        return Err(form());
    }
    // NAME < END or NAME <= END
    let test = test.strip_prefix(name).ok_or_else(form)?.trim_start();
    let (inclusive, end) = match test.strip_prefix("<=") {
        Some(end) => (1, end),
        None => (0, test.strip_prefix('<').ok_or_else(form)?),
    };
    let end = number(end).ok_or_else(form)? + inclusive;
    // NAME++ or NAME += STEP
    let step = step.strip_prefix(name).ok_or_else(form)?.trim();
    let step = match step {
        "++" => 1,
        _ => number(step.strip_prefix("+=").ok_or_else(form)?).ok_or_else(form)?,
    };
    if step < 1 {
        return Err(form());
    }
    let count = ((end - start).max(0) + step - 1) / step;
    if count > MAX_LOOP {
        return fail(format!(
            "line {line}: this loop runs {count} times; the most is {MAX_LOOP}"
        ));
    }
    let after = text[close + 1..].trim_start();
    let brace = text.len() - after.len();
    if !after.starts_with('{') {
        return fail(format!("line {line}: a loop's body goes in braces"));
    }
    let end = closing(text, brace).ok_or(format!("line {line}: the loop's {{ is never closed"))?;
    // The counter may only be read in the body, or the length would not be known.
    let body = &text[brace + 1..end];
    for i in words(body, name) {
        let rest = body[i + name.len()..].trim_start();
        let before = body[..i].trim_end();
        let written = (rest.starts_with('=') && !rest.starts_with("=="))
            || ["+=", "-=", "*=", "/=", "%=", "++", "--"]
                .iter()
                .any(|op| rest.starts_with(op))
            || before.ends_with('&');
        if written {
            return fail(format!(
                "line {}: the loop counter {name} may only be read inside its loop",
                line_of(text, brace + 1 + i)
            ));
        }
    }
    Ok(Loop {
        body: brace + 1..end,
        count,
    })
}
/// Whether `code` keeps to the rules that make a custom shader safe to run on every
/// pixel: bounded size, plain ASCII outside comments, a `shade` function, nothing
/// that reaches outside it (attributes, global variables), and loops only inside
/// `shade`, each of a fixed length with at most MAX_LOOP iterations when nested.
pub fn check(code: &str) -> Result<Vec<Control>, String> {
    if code.len() > MAX_SOURCE {
        return fail(format!("a shader is at most {MAX_SOURCE} characters"));
    }
    if code.contains('\0') {
        return fail("the shader contains a NUL character");
    }
    let declared = controls(code)?;
    let text = without_comments(code);
    if let Some(i) = text.find('?').filter(|&i| !code.as_bytes()[i].is_ascii()) {
        return fail(format!(
            "line {}: only plain ASCII is allowed outside comments",
            line_of(&text, i)
        ));
    }
    if let Some(i) = text.find('@') {
        return fail(format!(
            "line {}: attributes (@…) are not needed: the node provides the inputs and the entry point",
            line_of(&text, i)
        ));
    }
    for (word, why) in [
        ("while", "use a for loop of a fixed length instead of while"),
        ("loop", "use a for loop of a fixed length instead of loop"),
        (
            "continuing",
            "continuing belongs to loop, which is not allowed",
        ),
        (
            "var<private>",
            "global variables are not allowed; use const or values inside shade",
        ),
        ("enable", "extensions cannot be enabled"),
        ("requires", "language extensions cannot be required"),
    ] {
        let found = if word.contains('<') {
            text.find(word)
        } else {
            words(&text, word).next()
        };
        if let Some(i) = found {
            return fail(format!("line {}: {why}", line_of(&text, i)));
        }
    }
    // Top-level `var` (any address space) is a global; function bodies start after `{`.
    let mut depth = 0usize;
    let mut function = "";
    let mut functions: Vec<(&str, std::ops::Range<usize>)> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if depth == 0 && !function.is_empty() {
                    let end = closing(&text, i).ok_or("a { is never closed")?;
                    functions.push((function, i..end));
                    function = "";
                }
                depth += 1;
            }
            b'}' => depth = depth.saturating_sub(1),
            _ if depth == 0
                && text[i..].starts_with("var")
                && words(&text[i..], "var").next() == Some(0) =>
            {
                return fail(format!(
                    "line {}: global variables are not allowed; use const or values inside shade",
                    line_of(&text, i)
                ));
            }
            _ if depth == 0
                && text[i..].starts_with("fn")
                && words(&text[i..], "fn").next() == Some(0) =>
            {
                let rest = text[i + 2..].trim_start();
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(0);
                function = &rest[..end];
            }
            _ => {}
        }
        i += 1;
    }
    for reserved in ["custom", "image", "second", "texel_of", "sample_of"] {
        if functions.iter().any(|(name, _)| *name == reserved) {
            return fail(format!(
                "the name {reserved} is taken by the node; call your function something else"
            ));
        }
    }
    let Some((_, body)) = functions.iter().find(|(name, _)| *name == "shade") else {
        return fail("the shader needs fn shade(at: vec2<f32>) -> vec4<f32>");
    };
    // Its signature is fixed: one pixel position in, one colour out.
    let start = text[..body.start].rfind("fn").unwrap_or(0);
    let signature: String = text[start..body.start].split_whitespace().collect();
    let argument = signature
        .strip_prefix("fnshade(")
        .and_then(|rest| rest.strip_suffix(":vec2<f32>)->vec4<f32>"));
    if !argument.is_some_and(identifier) {
        return fail(format!(
            "line {}: shade must be written fn shade(at: vec2<f32>) -> vec4<f32>",
            line_of(&text, start)
        ));
    }
    let loops: Vec<usize> = words(&text, "for").collect();
    if loops.len() > MAX_LOOPS {
        return fail(format!("a shader has at most {MAX_LOOPS} loops"));
    }
    let mut parsed: Vec<Loop> = Vec::new();
    for at in loops {
        let inside = functions
            .iter()
            .find(|(_, body)| body.contains(&at))
            .map(|(name, _)| *name);
        if inside != Some("shade") {
            return fail(format!(
                "line {}: loops are only allowed inside shade, so their total length is known",
                line_of(&text, at)
            ));
        }
        let this = parse_loop(&text, at)?;
        let nested: i64 = parsed
            .iter()
            .filter(|outer| outer.body.contains(&at))
            .map(|outer| outer.count)
            .product::<i64>()
            * this.count;
        if nested > MAX_LOOP {
            return fail(format!(
                "line {}: nested loops run {nested} times in all; the most is {MAX_LOOP}",
                line_of(&text, at)
            ));
        }
        parsed.push(this);
    }
    Ok(declared)
}
/// The compute program around a checked shader, and how many lines come before the
/// user's first line (to turn compiler line numbers back into theirs).
pub fn assemble(code: &str, controls: &[Control]) -> (String, usize) {
    let mut fields = String::new();
    let mut fill = String::new();
    let slot = |i: usize| {
        let (vector, part) = (
            ["values", "extra", "third", "fourth"][i / 4],
            ["x", "y", "z", "w"][i % 4],
        );
        format!("p.{vector}.{part}")
    };
    for control in controls {
        match control {
            Control::Slider { name, slot: s, .. } => {
                let _ = writeln!(fields, "    {name}: f32,");
                let _ = writeln!(fill, "    u.{name} = {};", slot(*s));
            }
            Control::Colour { name, slot: s, .. } => {
                let _ = writeln!(fields, "    {name}: vec3<f32>,");
                let _ = writeln!(
                    fill,
                    "    u.{name} = vec3<f32>({}, {}, {});",
                    slot(*s),
                    slot(*s + 1),
                    slot(*s + 2)
                );
            }
        }
    }
    if controls.is_empty() {
        fields.push_str("    unused: f32,\n");
    }
    let prelude = format!(
        "struct Params {{ size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32>, third: vec4<f32>, fourth: vec4<f32>, anchor: vec4<f32> }}
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> other: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>>;
struct Controls {{
{fields}}}
var<private> u: Controls;
var<private> size: vec2<f32>;
var<private> time: f32;
var<private> follow: vec2<f32>;
var<private> followed: f32;
fn texel_of(which: u32, q: vec2<i32>) -> vec4<f32> {{
    let s = vec2<i32>(p.size_op.xy);
    if (p.size_op.w & which) == 0u || q.x < 0 || q.y < 0 || q.x >= s.x || q.y >= s.y {{ return vec4<f32>(0.0); }}
    let i = u32(q.y) * p.size_op.x + u32(q.x);
    if which == 1u {{ return source[i]; }}
    return other[i];
}}
fn sample_of(which: u32, at: vec2<f32>) -> vec4<f32> {{
    let q = at - vec2<f32>(0.5);
    let base = vec2<i32>(floor(q));
    let f = q - floor(q);
    return mix(mix(texel_of(which, base), texel_of(which, base + vec2<i32>(1, 0)), f.x),
               mix(texel_of(which, base + vec2<i32>(0, 1)), texel_of(which, base + vec2<i32>(1, 1)), f.x), f.y);
}}
fn image(at: vec2<f32>) -> vec4<f32> {{ return sample_of(1u, at); }}
fn second(at: vec2<f32>) -> vec4<f32> {{ return sample_of(2u, at); }}
"
    );
    let lines = prelude.matches('\n').count();
    let program = format!(
        "{prelude}{code}
@compute @workgroup_size(8, 8)
fn custom(@builtin(global_invocation_id) gid: vec3<u32>) {{
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y {{ return; }}
    size = vec2<f32>(p.size_op.xy);
    time = bitcast<f32>(p.size_op.z);
    follow = p.anchor.xy;
    followed = p.anchor.z;
{fill}    result[gid.y * p.size_op.x + gid.x] = shade(vec2<f32>(gid.xy) + vec2<f32>(0.5));
}}
"
    );
    (program, lines)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_example_declares_its_controls_and_defaults() {
        let declared = check(EXAMPLE).unwrap();
        assert_eq!(declared.len(), 2);
        assert_eq!(defaults(&declared), EXAMPLE_DEFAULTS);
        let (program, lines) = assemble(EXAMPLE, &declared);
        assert!(program.contains("u.amount = p.values.x;"));
        assert!(program.contains("u.tint = vec3<f32>(p.values.y, p.values.z, p.values.w);"));
        assert_eq!(program.lines().nth(lines), EXAMPLE.lines().next());
    }
    #[test]
    fn declarations_are_checked() {
        for (code, hint) in [
            ("// slider a 0 1\nfn shade", "MIN MAX DEFAULT"),
            ("// slider a 1 0 0.5", "MIN below MAX"),
            ("// slider a 0 1 0.5\n// slider a 0 1 0.5", "two controls"),
            ("// colour c 1 2", "RED GREEN BLUE"),
            ("// slider 9a 0 1 0", "needs a name"),
            (
                "// colour a 1 1 1\n// colour b 1 1 1\n// colour c 1 1 1\n// colour d 1 1 1\n// colour e 1 1 1\n// colour f 1 1 1",
                "control values",
            ),
        ] {
            let error = controls(code).unwrap_err();
            assert!(error.contains(hint), "{code}: {error}");
        }
    }
    #[test]
    fn only_bounded_self_contained_code_passes() {
        let shade = |body: &str| {
            format!("fn shade(at: vec2<f32>) -> vec4<f32> {{\n{body}\nreturn image(at);\n}}\n")
        };
        for good in [
            shade(
                "var s = vec4<f32>(0.0);\nfor (var i = 0; i < 16; i++) { for (var j: i32 = -8; j <= 8; j += 2) { s += image(at + vec2<f32>(f32(i), f32(j))); } }",
            ),
            format!(
                "const K = 3.0;\nfn helper(x: f32) -> f32 {{ return x * K; }}\n{}",
                shade("let k = helper(1.0); // while we wait @here")
            ),
        ] {
            check(&good).unwrap_or_else(|e| panic!("{e}\n{good}"));
        }
        for (bad, hint) in [
            (shade("while (true) { }"), "instead of while"),
            (shade("loop { }"), "instead of loop"),
            (
                shade("for (var i = 0; i < 2000; i++) { }"),
                "runs 2000 times",
            ),
            (
                shade("for (var i = 0; i < 64; i++) { for (var j = 0; j < 64; j++) { } }"),
                "4096 times in all",
            ),
            (
                shade("for (var i = 0; i < 8; i++) { i = 0; }"),
                "only be read",
            ),
            (
                shade("for (var i = 0; i < 8; i++) { i -= 1; }"),
                "only be read",
            ),
            (shade("for (var i = 0; i < n; i++) { }"), "whole numbers"),
            (shade("for (;;) { }"), "whole numbers"),
            (
                format!(
                    "fn helper() {{ for (var i = 0; i < 4; i++) {{ }} }}\n{}",
                    shade("")
                ),
                "only allowed inside shade",
            ),
            (
                format!("@group(0) @binding(9) var x: f32;\n{}", shade("")),
                "attributes",
            ),
            (
                format!("var<private> g: f32;\n{}", shade("")),
                "global variables",
            ),
            (format!("var g: f32;\n{}", shade("")), "global variables"),
            (
                "fn other() -> f32 { return 1.0; }".to_string(),
                "needs fn shade",
            ),
            (
                "fn shade(at: vec2<f32>) -> f32 { return 1.0; }".to_string(),
                "must be written",
            ),
            (
                "fn shade(at: vec2<f32>, b: f32) -> vec4<f32> { return image(at); }".to_string(),
                "must be written",
            ),
            (
                format!(
                    "fn image(at: vec2<f32>) -> f32 {{ return 1.0; }}\n{}",
                    shade("")
                ),
                "taken by the node",
            ),
            (shade("let é = 1.0;"), "plain ASCII"),
            ("x".repeat(MAX_SOURCE + 1), "at most"),
        ] {
            let error = check(&bad).unwrap_err();
            assert!(error.contains(hint), "{error}\n{bad}");
        }
    }
}
