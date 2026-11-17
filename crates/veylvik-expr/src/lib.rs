//! Bounded arithmetic expressions for animated parameters: numbers, `time`, `frame`, + - * / % ^, a fixed set of functions and links `[Node].parameter` to other parameters. Source, program length, nesting and links are all capped; evaluation runs a flat program on a fixed stack, with no recursion.
use std::fmt;

/// Longest accepted source text, in bytes.
pub const MAX_SOURCE: usize = 1000;
/// Most program steps (numbers, names, operators and calls).
pub const MAX_OPS: usize = 256;
/// Most links in one expression.
pub const MAX_LINKS: usize = 16;
/// Deepest nesting of parentheses, calls and operators.
const MAX_DEPTH: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}

/// A link to another parameter, `[node].parameter`, with where its node text sits in
/// the source (so it can be rewritten, such as a name to an id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub node: String,
    pub parameter: String,
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Number(f64),
    Time,
    Frame,
    Link(usize),
    Negate,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Power,
    Call(Function),
}

macro_rules! functions {
    ($($variant:ident $name:literal $arity:literal),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Function { $($variant),* }
        impl Function {
            fn named(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)* _ => None }
            }
            fn arity(self) -> usize {
                match self { $(Self::$variant => $arity),* }
            }
        }
        /// Every function, as `name(arguments)`, for help text.
        pub const FUNCTIONS: &[(&str, usize)] = &[$(($name, $arity)),*];
    };
}
functions! {
    Sin "sin" 1, Cos "cos" 1, Tan "tan" 1, Asin "asin" 1, Acos "acos" 1, Atan "atan" 1,
    Atan2 "atan2" 2, Sqrt "sqrt" 1, Abs "abs" 1, Floor "floor" 1, Ceil "ceil" 1,
    Round "round" 1, Fract "fract" 1, Sign "sign" 1, Exp "exp" 1, Ln "ln" 1,
    Log10 "log10" 1, Pow "pow" 2, Min "min" 2, Max "max" 2, Clamp "clamp" 3, Mix "mix" 3,
    Step "step" 2, Smoothstep "smoothstep" 3, Mod "mod" 2, Noise "noise" 1,
    Random "random" 1, Radians "radians" 1, Degrees "degrees" 1,
}
impl Function {
    fn apply(self, a: &[f64]) -> f64 {
        match self {
            Self::Sin => a[0].sin(),
            Self::Cos => a[0].cos(),
            Self::Tan => a[0].tan(),
            Self::Asin => a[0].asin(),
            Self::Acos => a[0].acos(),
            Self::Atan => a[0].atan(),
            Self::Atan2 => a[0].atan2(a[1]),
            Self::Sqrt => a[0].sqrt(),
            Self::Abs => a[0].abs(),
            Self::Floor => a[0].floor(),
            Self::Ceil => a[0].ceil(),
            Self::Round => a[0].round(),
            Self::Fract => a[0] - a[0].floor(),
            Self::Sign => {
                if a[0] == 0.0 {
                    0.0
                } else {
                    a[0].signum()
                }
            }
            Self::Exp => a[0].exp(),
            Self::Ln => a[0].ln(),
            Self::Log10 => a[0].log10(),
            Self::Pow => a[0].powf(a[1]),
            Self::Min => a[0].min(a[1]),
            Self::Max => a[0].max(a[1]),
            Self::Clamp => a[0].max(a[1]).min(a[2]),
            Self::Mix => a[0] + (a[1] - a[0]) * a[2],
            Self::Step => f64::from(u8::from(a[1] >= a[0])),
            Self::Smoothstep => {
                let t = ((a[2] - a[0]) / (a[1] - a[0])).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
            Self::Mod => a[0] - a[1] * (a[0] / a[1]).floor(),
            Self::Noise => noise(a[0]),
            Self::Random => unit(lattice(a[0].floor())),
            Self::Radians => a[0].to_radians(),
            Self::Degrees => a[0].to_degrees(),
        }
    }
}
/// A well-mixed 64-bit hash of a lattice point.
fn lattice(x: f64) -> u64 {
    let mut h = (x as i64 as u64) ^ 0x9e37_79b9_7f4a_7c15;
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}
/// Smooth gradient noise in -1..=1, zero at whole numbers, about one wiggle per unit.
fn noise(x: f64) -> f64 {
    if !x.is_finite() || x.abs() > 1e15 {
        return f64::NAN;
    }
    let i = x.floor();
    let f = x - i;
    let g0 = unit(lattice(i)) * 2.0 - 1.0;
    let g1 = unit(lattice(i + 1.0)) * 2.0 - 1.0;
    let s = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    (2.0 * (g0 * f * (1.0 - s) + g1 * (f - 1.0) * s)).clamp(-1.0, 1.0)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Link(Link),
    Symbol(char),
}
fn tokens(source: &str) -> Result<Vec<(Token, usize)>> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = source.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|n| n.1.is_ascii_digit()))
        {
            let start = i;
            while i < chars.len() && (chars[i].1.is_ascii_digit() || chars[i].1 == '.') {
                i += 1;
            }
            if i < chars.len() && matches!(chars[i].1, 'e' | 'E') {
                let mut j = i + 1;
                if j < chars.len() && matches!(chars[j].1, '+' | '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].1.is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].1.is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let end = chars.get(i).map_or(source.len(), |c| c.0);
            let text = &source[chars[start].0..end];
            let value: f64 = text
                .parse()
                .map_err(|_| Error(format!("'{text}' is not a number")))?;
            out.push((Token::Number(value), at));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].1.is_alphanumeric() || chars[i].1 == '_') {
                i += 1;
            }
            let end = chars.get(i).map_or(source.len(), |c| c.0);
            out.push((Token::Name(source[chars[start].0..end].to_string()), at));
        } else if c == '[' {
            let Some(close) = (i + 1..chars.len()).find(|&j| chars[j].1 == ']') else {
                return fail("a link's [ has no closing ]");
            };
            let (from, to) = (chars[i].0 + 1, chars[close].0);
            let node = source[from..to].trim();
            if node.is_empty() || node.len() > 200 {
                return fail("a link names its node between [ and ], such as [Title].size");
            }
            let lead = source[from..to].len() - source[from..to].trim_start().len();
            let span = from + lead..from + lead + node.len();
            i = close + 1;
            if chars.get(i).map(|c| c.1) != Some('.') {
                return fail(format!(
                    "[{node}] needs a parameter after a dot, such as [{node}].size"
                ));
            }
            i += 1;
            let start = i;
            while i < chars.len() && (chars[i].1.is_alphanumeric() || chars[i].1 == '_') {
                i += 1;
            }
            if start == i {
                return fail(format!("[{node}]. needs a parameter name"));
            }
            let end = chars.get(i).map_or(source.len(), |c| c.0);
            out.push((
                Token::Link(Link {
                    node: node.to_string(),
                    parameter: source[chars[start].0..end].to_string(),
                    span,
                }),
                at,
            ));
        } else if "+-*/%^(),".contains(c) {
            out.push((Token::Symbol(c), at));
            i += 1;
        } else {
            return fail(format!("'{c}' is not part of an expression"));
        }
    }
    Ok(out)
}

/// A parsed expression, ready to evaluate any number of times.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    ops: Vec<Op>,
    links: Vec<Link>,
    stack: usize,
}
struct Parser {
    tokens: Vec<(Token, usize)>,
    at: usize,
    ops: Vec<Op>,
    links: Vec<Link>,
}
impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|t| &t.0)
    }
    fn symbol(&mut self, c: char) -> bool {
        if self.peek() == Some(&Token::Symbol(c)) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn emit(&mut self, op: Op) -> Result<()> {
        if self.ops.len() >= MAX_OPS {
            return fail(format!("the expression is too long (over {MAX_OPS} steps)"));
        }
        self.ops.push(op);
        Ok(())
    }
    fn deeper(depth: usize) -> Result<usize> {
        if depth >= MAX_DEPTH {
            return fail("the expression is nested too deeply");
        }
        Ok(depth + 1)
    }
    fn sum(&mut self, depth: usize) -> Result<()> {
        let depth = Self::deeper(depth)?;
        self.product(depth)?;
        loop {
            let op = if self.symbol('+') {
                Op::Add
            } else if self.symbol('-') {
                Op::Subtract
            } else {
                return Ok(());
            };
            self.product(depth)?;
            self.emit(op)?;
        }
    }
    fn product(&mut self, depth: usize) -> Result<()> {
        let depth = Self::deeper(depth)?;
        self.unary(depth)?;
        loop {
            let op = if self.symbol('*') {
                Op::Multiply
            } else if self.symbol('/') {
                Op::Divide
            } else if self.symbol('%') {
                Op::Remainder
            } else {
                return Ok(());
            };
            self.unary(depth)?;
            self.emit(op)?;
        }
    }
    /// Unary minus binds looser than ^, so -2^2 is -4.
    fn unary(&mut self, depth: usize) -> Result<()> {
        let depth = Self::deeper(depth)?;
        if self.symbol('-') {
            self.unary(depth)?;
            return self.emit(Op::Negate);
        }
        if self.symbol('+') {
            return self.unary(depth);
        }
        self.atom(depth)?;
        if self.symbol('^') {
            self.unary(depth)?;
            self.emit(Op::Power)?;
        }
        Ok(())
    }
    fn atom(&mut self, depth: usize) -> Result<()> {
        let depth = Self::deeper(depth)?;
        let Some((token, _)) = self.tokens.get(self.at).cloned() else {
            return fail("the expression ends too early");
        };
        self.at += 1;
        match token {
            Token::Number(n) => self.emit(Op::Number(n)),
            Token::Link(link) => {
                if self.links.len() >= MAX_LINKS {
                    return fail(format!(
                        "an expression links at most {MAX_LINKS} parameters"
                    ));
                }
                let index = match self
                    .links
                    .iter()
                    .position(|l| l.node == link.node && l.parameter == link.parameter)
                {
                    Some(i) => {
                        self.links.push(link);
                        i
                    }
                    None => {
                        self.links.push(link);
                        self.links.len() - 1
                    }
                };
                self.emit(Op::Link(index))
            }
            Token::Symbol('(') => {
                self.sum(depth)?;
                if !self.symbol(')') {
                    return fail("a ( has no closing )");
                }
                Ok(())
            }
            Token::Symbol(c) => fail(format!("'{c}' cannot start a value")),
            Token::Name(name) => {
                if self.symbol('(') {
                    let Some(function) = Function::named(&name) else {
                        return fail(format!("'{name}' is not a function"));
                    };
                    let mut count = 0;
                    if !self.symbol(')') {
                        loop {
                            self.sum(depth)?;
                            count += 1;
                            if self.symbol(')') {
                                break;
                            }
                            if !self.symbol(',') {
                                return fail(format!("{name}( needs a comma or a closing )"));
                            }
                        }
                    }
                    if count != function.arity() {
                        return fail(format!(
                            "{name} takes {} value(s), not {count}",
                            function.arity()
                        ));
                    }
                    return self.emit(Op::Call(function));
                }
                match name.as_str() {
                    "time" => self.emit(Op::Time),
                    "frame" => self.emit(Op::Frame),
                    "pi" => self.emit(Op::Number(std::f64::consts::PI)),
                    "tau" => self.emit(Op::Number(std::f64::consts::TAU)),
                    _ if Function::named(&name).is_some() => {
                        fail(format!("{name} is a function: write {name}(…)"))
                    }
                    _ => fail(format!(
                        "'{name}' is unknown: use time, frame, pi, tau, a function or a link such as [Title].size"
                    )),
                }
            }
        }
    }
}
/// Parse an expression.
pub fn parse(source: &str) -> Result<Program> {
    if source.len() > MAX_SOURCE {
        return fail(format!("an expression is at most {MAX_SOURCE} characters"));
    }
    let tokens = tokens(source)?;
    if tokens.is_empty() {
        return fail("the expression is empty");
    }
    let mut parser = Parser {
        tokens,
        at: 0,
        ops: Vec::new(),
        links: Vec::new(),
    };
    parser.sum(0)?;
    if let Some((token, _)) = parser.tokens.get(parser.at) {
        return fail(match token {
            Token::Symbol(')') => "a ) has no opening (".to_string(),
            _ => "two values need an operator between them".to_string(),
        });
    }
    // The stack never holds more values than there are steps; measure it once.
    let mut depth: usize = 0;
    let mut stack = 0;
    for op in &parser.ops {
        depth = match op {
            Op::Number(_) | Op::Time | Op::Frame | Op::Link(_) => depth + 1,
            Op::Negate => depth,
            Op::Call(f) => depth + 1 - f.arity(),
            _ => depth - 1,
        };
        stack = stack.max(depth);
    }
    // Repeated links share one slot; keep each link's span for rewriting.
    Ok(Program {
        ops: parser.ops,
        links: parser.links,
        stack,
    })
}
impl Program {
    /// Every link as written, in order of appearance (repeats included, so each
    /// one's span can be rewritten); `eval` asks for them by index into this list.
    pub fn links(&self) -> &[Link] {
        &self.links
    }
    /// Evaluate at `time` seconds and `frame`, asking `link` for linked values.
    pub fn eval(
        &self,
        time: f64,
        frame: f64,
        link: &mut dyn FnMut(usize) -> Result<f64>,
    ) -> Result<f64> {
        let mut stack: Vec<f64> = Vec::with_capacity(self.stack);
        for op in &self.ops {
            let value = match *op {
                Op::Number(n) => n,
                Op::Time => time,
                Op::Frame => frame,
                Op::Link(i) => link(i)?,
                Op::Call(f) => {
                    let n = f.arity();
                    let at = stack.len() - n;
                    let value = f.apply(&stack[at..]);
                    stack.truncate(at);
                    value
                }
                Op::Negate => -stack.pop().unwrap_or(0.0),
                binary => {
                    let b = stack.pop().unwrap_or(0.0);
                    let a = stack.pop().unwrap_or(0.0);
                    match binary {
                        Op::Add => a + b,
                        Op::Subtract => a - b,
                        Op::Multiply => a * b,
                        Op::Divide => a / b,
                        Op::Remainder => a % b,
                        _ => a.powf(b),
                    }
                }
            };
            stack.push(value);
        }
        let value = stack.pop().unwrap_or(f64::NAN);
        if value.is_finite() {
            Ok(value)
        } else {
            fail(format!(
                "the expression gives no number at {time:.3} s (a division by zero or a square root of a negative?)"
            ))
        }
    }
}
/// The source with every link's node text replaced by `rename` (for example a node's
/// name by its id); links `rename` returns None for are left as written.
pub fn relink(source: &str, mut rename: impl FnMut(&str) -> Option<String>) -> Result<String> {
    let program = parse(source)?;
    let mut out = String::with_capacity(source.len());
    let mut last = 0;
    for link in program.links() {
        out.push_str(&source[last..link.span.start]);
        out.push_str(&rename(&link.node).unwrap_or_else(|| link.node.clone()));
        last = link.span.end;
    }
    out.push_str(&source[last..]);
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn value(source: &str) -> f64 {
        parse(source)
            .unwrap()
            .eval(2.0, 48.0, &mut |_| Ok(10.0))
            .unwrap()
    }
    #[test]
    fn arithmetic_follows_the_usual_rules() {
        assert_eq!(value("1 + 2 * 3"), 7.0);
        assert_eq!(value("(1 + 2) * 3"), 9.0);
        assert_eq!(value("-2^2"), -4.0);
        assert_eq!(value("2^3^2"), 512.0);
        assert_eq!(value("7 % 4 + 10 / 4"), 5.5);
        assert_eq!(value("time * frame"), 96.0);
        assert_eq!(value("clamp(5, 0, 1) + min(3, 4) + mix(0, 10, 0.25)"), 6.5);
        assert_eq!(value("1.5e2 + .5"), 150.5);
        assert!((value("sin(pi / 2)") - 1.0).abs() < 1e-12);
        assert_eq!(value("[Title].size * 2 + [Title].size"), 30.0);
        assert_eq!(value("mod(-1, 3)"), 2.0);
    }
    #[test]
    fn noise_is_smooth_bounded_and_repeatable() {
        let mut previous = noise(0.0);
        for i in 1..2000 {
            let n = noise(f64::from(i) * 0.01);
            assert!((-1.0..=1.0).contains(&n));
            assert!((n - previous).abs() < 0.05, "smooth");
            previous = n;
        }
        assert_eq!(noise(3.7), noise(3.7));
        assert_eq!(noise(4.0), 0.0);
        assert!((0.0..1.0).contains(&value("random(frame)")));
    }
    #[test]
    fn mistakes_are_explained() {
        for (source, hint) in [
            ("1 +", "ends too early"),
            ("sin(1, 2)", "takes 1"),
            ("tme", "unknown"),
            ("(1", "closing )"),
            ("1 2", "operator"),
            ("[Title]", "parameter after a dot"),
            ("2 $ 3", "not part"),
            ("sin", "is a function"),
            ("", "empty"),
        ] {
            let error = parse(source).unwrap_err().0;
            assert!(error.contains(hint), "{source}: {error}");
        }
        assert!(parse(&"(".repeat(60)).unwrap_err().0.contains("nested"));
        assert!(
            parse(&"1+".repeat(200).add_one())
                .unwrap_err()
                .0
                .contains("too long")
        );
        assert!(
            parse("1 / 0")
                .unwrap()
                .eval(0.0, 0.0, &mut |_| Ok(0.0))
                .is_err()
        );
    }
    trait AddOne {
        fn add_one(self) -> String;
    }
    impl AddOne for String {
        fn add_one(self) -> String {
            self + "1"
        }
    }
    #[test]
    fn links_can_be_renamed_in_place() {
        let out = relink("[ Title ].size + [Glow].amount*[Title].x", |n| {
            (n == "Title").then(|| "1234".to_string())
        })
        .unwrap();
        assert_eq!(out, "[ 1234 ].size + [Glow].amount*[1234].x");
    }
}
