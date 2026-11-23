//! A Nuke script read as Nuke reads it: node blocks of knobs, wired by a stack (each node takes its inputs from the most recent pushes, the most recent as input 0; `set` names the top, `push` puts a named node or nothing back).

/// A knob's value as written: a word or quoted string, or a braced group (kept as its
/// inner text, to be read as numbers, a list or a curve).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Word(String),
    Group(String),
}
impl Value {
    pub fn text(&self) -> &str {
        match self {
            Value::Word(w) | Value::Group(w) => w,
        }
    }
}
/// A node as the script has it: its class, knobs in order, and its inputs (indices
/// into the script's nodes; None where nothing is connected).
#[derive(Debug, Clone)]
pub struct Read {
    pub class: String,
    pub knobs: Vec<(String, Value)>,
    pub inputs: Vec<Option<usize>>,
}
impl Read {
    pub fn knob(&self, name: &str) -> Option<&Value> {
        self.knobs.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn name(&self) -> String {
        self.knob("name")
            .map_or_else(|| self.class.clone(), |v| v.text().to_string())
    }
}
/// The script's nodes in order, its Root settings, and groups left closed.
#[derive(Debug, Default)]
pub struct Script {
    pub root: Vec<(String, Value)>,
    pub nodes: Vec<Read>,
    /// Groups whose insides were not read (each a node of its own here).
    pub closed_groups: usize,
}
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Group(String),
    Open,
    Close,
    Line,
}
/// Splits the script into words, quoted strings, braced groups and line ends; a group
/// directly after a knob name is kept whole, as its value.
fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    // Depth of node blocks we are inside, to tell a node's body from a knob's value.
    let mut in_body = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                out.push(Token::Line);
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '#' if matches!(out.last(), None | Some(Token::Line)) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '"' => {
                let mut word = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                        word.push(match chars[i] {
                            'n' => '\n',
                            other => other,
                        });
                    } else {
                        word.push(chars[i]);
                    }
                    i += 1;
                }
                i += 1;
                out.push(Token::Word(word));
            }
            '{' => {
                // A knob's value when it follows a knob name inside a node's body.
                let value = in_body > 0 && matches!(out.last(), Some(Token::Word(_)));
                if value {
                    let mut depth = 1;
                    let mut inner = String::new();
                    i += 1;
                    while i < chars.len() && depth > 0 {
                        match chars[i] {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        if depth > 0 {
                            inner.push(chars[i]);
                        }
                        i += 1;
                    }
                    if depth > 0 {
                        return Err("a brace is never closed".into());
                    }
                    out.push(Token::Group(inner.trim().to_string()));
                } else {
                    in_body += 1;
                    out.push(Token::Open);
                    i += 1;
                }
            }
            '}' => {
                in_body = in_body.saturating_sub(1);
                out.push(Token::Close);
                i += 1;
            }
            _ => {
                let mut word = String::new();
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], '{' | '}' | '"')
                {
                    word.push(chars[i]);
                    i += 1;
                }
                out.push(Token::Word(word));
            }
        }
    }
    if in_body > 0 {
        return Err("a node is never closed: not a whole Nuke script".into());
    }
    Ok(out)
}
/// Reads a script.
pub fn read(text: &str) -> Result<Script, String> {
    let tokens = tokens(text)?;
    let mut script = Script::default();
    let mut stack: Vec<Option<usize>> = Vec::new();
    let mut names: std::collections::BTreeMap<String, Option<usize>> = Default::default();
    let mut i = 0;
    let word = |t: Option<&Token>| match t {
        Some(Token::Word(w)) => Some(w.clone()),
        _ => None,
    };
    while i < tokens.len() {
        match &tokens[i] {
            Token::Line => i += 1,
            Token::Word(w) if w == "set" => {
                // set NAME [stack 0]
                let name = word(tokens.get(i + 1)).ok_or("a set without a name")?;
                names.insert(name, stack.last().copied().flatten());
                while i < tokens.len() && tokens[i] != Token::Line {
                    i += 1;
                }
            }
            Token::Word(w) if w == "push" => {
                let what = word(tokens.get(i + 1)).ok_or("a push without a value")?;
                let node = match what.strip_prefix('$') {
                    Some(name) => names.get(name).copied().flatten(),
                    None => None,
                };
                stack.push(node);
                i += 2;
            }
            Token::Word(w) if w == "version" || w == "define_window_layout_xml" => {
                while i < tokens.len() && tokens[i] != Token::Line {
                    i += 1;
                }
            }
            Token::Word(w) if w == "end_group" => i += 1,
            Token::Word(class) if matches!(tokens.get(i + 1), Some(Token::Open)) => {
                let class = class.clone();
                i += 2;
                let mut knobs = Vec::new();
                while i < tokens.len() && tokens[i] != Token::Close {
                    match (&tokens[i], tokens.get(i + 1)) {
                        (Token::Line, _) => i += 1,
                        (Token::Word(name), Some(Token::Word(v))) => {
                            knobs.push((name.clone(), Value::Word(v.clone())));
                            i += 2;
                        }
                        (Token::Word(name), Some(Token::Group(v))) => {
                            knobs.push((name.clone(), Value::Group(v.clone())));
                            i += 2;
                        }
                        _ => i += 1,
                    }
                }
                i += 1;
                if class == "Root" {
                    script.root = knobs;
                    continue;
                }
                let node = Read {
                    class: class.clone(),
                    knobs,
                    inputs: Vec::new(),
                };
                // Inputs from the stack: the most recent is input 0, the one before it
                // input 1, and so on (on a Merge, B and then A).
                let count = match node.knob("inputs").map(|v| v.text().parse::<usize>()) {
                    Some(Ok(n)) => n,
                    _ if matches!(
                        class.as_str(),
                        "Read"
                            | "Constant"
                            | "ColorWheel"
                            | "CheckerBoard2"
                            | "StickyNote"
                            | "BackdropNode"
                            | "Camera"
                            | "Camera2"
                            | "Camera3"
                            | "Axis"
                            | "Axis2"
                    ) =>
                    {
                        0
                    }
                    _ => 1,
                };
                let taken = stack.len().saturating_sub(count);
                let mut inputs: Vec<Option<usize>> = stack.split_off(taken);
                inputs.reverse();
                inputs.resize(count, None);
                let index = script.nodes.len();
                script.nodes.push(Read { inputs, ..node });
                if !matches!(class.as_str(), "StickyNote" | "BackdropNode") {
                    stack.push(Some(index));
                }
                // A group's insides are left closed: skip to its end_group.
                if class == "Group" || class == "Gizmo" || class == "LiveGroup" {
                    script.closed_groups += 1;
                    let mut depth = 1;
                    while i < tokens.len() && depth > 0 {
                        match &tokens[i] {
                            Token::Word(w) if w == "end_group" => depth -= 1,
                            Token::Word(w)
                                if matches!(w.as_str(), "Group" | "Gizmo" | "LiveGroup")
                                    && matches!(tokens.get(i + 1), Some(Token::Open)) =>
                            {
                                depth += 1
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
            }
            _ => i += 1,
        }
    }
    Ok(script)
}
