//! Topology edits preserve untouched contours and split cubic curves exactly.
use veylvik_document::vector::{PathCommand, PathMask};
type Point = [f32; 2];
#[derive(Clone, Copy)]
pub enum Action {
    Insert,
    Delete,
    Smooth,
    Corner,
}
#[derive(Clone, Copy)]
struct Vertex {
    p: Point,
    incoming: Point,
    outgoing: Point,
}
fn mix(a: Point, b: Point, t: f32) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
fn vertex(p: Point) -> Vertex {
    Vertex {
        p,
        incoming: p,
        outgoing: p,
    }
}

/// The command identifies an anchor; duplicate closing endpoints alias the first.
pub fn edit(path: &mut PathMask, command: usize, action: Action) -> bool {
    if command >= path.commands.len() {
        return false;
    }
    let Some(start) = path.commands[..=command]
        .iter()
        .rposition(|c| matches!(c, PathCommand::Move { .. }))
    else {
        return false;
    };
    let Some(end) = path.commands[start..]
        .iter()
        .position(|c| matches!(c, PathCommand::Close {}))
        .map(|i| i + start)
    else {
        return false;
    };
    let PathCommand::Move { to } = path.commands[start] else {
        return false;
    };
    let mut vertices = vec![vertex(to)];
    let mut selected = 0;
    for i in start + 1..end {
        let previous = vertices.last().unwrap().p;
        let (c1, c2, to) = match path.commands[i] {
            PathCommand::Line { to } => (previous, to, to),
            PathCommand::Quad { control, to } => (
                mix(previous, control, 2.0 / 3.0),
                mix(to, control, 2.0 / 3.0),
                to,
            ),
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => (control1, control2, to),
            _ => return false,
        };
        vertices.last_mut().unwrap().outgoing = c1;
        if i + 1 == end && to == vertices[0].p {
            vertices[0].incoming = c2;
            if i == command {
                selected = 0;
            }
        } else {
            let mut v = vertex(to);
            v.incoming = c2;
            vertices.push(v);
            if i == command {
                selected = vertices.len() - 1;
            }
        }
    }
    if vertices.len() < 3 {
        return false;
    }
    let n = vertices.len();
    let next = (selected + 1) % n;
    let prev = (selected + n - 1) % n;
    match action {
        Action::Insert => {
            let a = vertices[selected];
            let b = vertices[next];
            let ab = mix(a.p, a.outgoing, 0.5);
            let bc = mix(a.outgoing, b.incoming, 0.5);
            let cd = mix(b.incoming, b.p, 0.5);
            let abc = mix(ab, bc, 0.5);
            let bcd = mix(bc, cd, 0.5);
            let p = mix(abc, bcd, 0.5);
            vertices[selected].outgoing = ab;
            vertices[next].incoming = cd;
            vertices.insert(
                selected + 1,
                Vertex {
                    p,
                    incoming: abc,
                    outgoing: bcd,
                },
            );
        }
        Action::Delete => {
            if n <= 3 {
                return false;
            }
            vertices.remove(selected);
        }
        Action::Corner => {
            if vertices[selected].incoming == vertices[selected].p
                && vertices[selected].outgoing == vertices[selected].p
            {
                return false;
            }
            vertices[selected].incoming = vertices[selected].p;
            vertices[selected].outgoing = vertices[selected].p;
        }
        Action::Smooth => {
            let p = vertices[selected].p;
            let a = vertices[prev].p;
            let b = vertices[next].p;
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let length = dx.hypot(dy);
            if length < 0.0001 {
                return false;
            }
            let incoming = (p[0] - a[0]).hypot(p[1] - a[1]) / 3.0;
            let outgoing = (b[0] - p[0]).hypot(b[1] - p[1]) / 3.0;
            vertices[selected].incoming =
                [p[0] - dx / length * incoming, p[1] - dy / length * incoming];
            vertices[selected].outgoing =
                [p[0] + dx / length * outgoing, p[1] + dy / length * outgoing];
        }
    }
    let mut commands = vec![PathCommand::Move { to: vertices[0].p }];
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        commands.push(PathCommand::Cubic {
            control1: a.outgoing,
            control2: b.incoming,
            to: b.p,
        });
    }
    commands.push(PathCommand::Close {});
    let mut result = path.clone();
    result.commands.splice(start..=end, commands);
    if result.validate().is_err() || result == *path {
        return false;
    }
    *path = result;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn evaluate(p: [Point; 4], t: f32) -> Point {
        mix(
            mix(mix(p[0], p[1], t), mix(p[1], p[2], t), t),
            mix(mix(p[1], p[2], t), mix(p[2], p[3], t), t),
            t,
        )
    }
    #[test]
    fn insertion_preserves_every_sample_of_cubic_and_other_contours() {
        let mut path = PathMask::default();
        path.commands[1] = PathCommand::Cubic {
            control1: [120.0, 20.0],
            control2: [350.0, 240.0],
            to: [400.0, 100.0],
        };
        let second = PathMask::default().commands;
        path.commands.extend(second.clone());
        assert!(edit(&mut path, 0, Action::Insert));
        let PathCommand::Cubic {
            control1: a,
            control2: b,
            to: c,
        } = path.commands[1]
        else {
            panic!()
        };
        let PathCommand::Cubic {
            control1: d,
            control2: e,
            to: f,
        } = path.commands[2]
        else {
            panic!()
        };
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            let expected = evaluate(
                [
                    [100.0, 100.0],
                    [120.0, 20.0],
                    [350.0, 240.0],
                    [400.0, 100.0],
                ],
                t,
            );
            let actual = if t <= 0.5 {
                evaluate([[100.0, 100.0], a, b, c], t * 2.0)
            } else {
                evaluate([c, d, e, f], (t - 0.5) * 2.0)
            };
            assert!(
                (expected[0] - actual[0]).abs() < 0.001 && (expected[1] - actual[1]).abs() < 0.001
            );
        }
        assert_eq!(
            &path.commands[path.commands.len() - second.len()..],
            &second
        );
    }
    #[test]
    fn first_anchor_smoothing_corner_and_deletion_keep_closed_valid_contours() {
        let mut path = PathMask::default();
        assert!(edit(&mut path, 0, Action::Smooth));
        path.validate().unwrap();
        let PathCommand::Cubic { control1, .. } = path.commands[1] else {
            panic!()
        };
        assert_ne!(control1, [100.0, 100.0]);
        assert!(edit(&mut path, 0, Action::Corner));
        let PathCommand::Cubic { control1, .. } = path.commands[1] else {
            panic!()
        };
        assert_eq!(control1, [100.0, 100.0]);
        assert!(edit(&mut path, 0, Action::Delete));
        path.validate().unwrap();
        let before = path.clone();
        assert!(!edit(&mut path, 0, Action::Delete));
        assert_eq!(path, before);
    }
    #[test]
    fn unchanged_corner_is_not_an_edit() {
        let mut path = PathMask::default();
        let before = path.clone();
        assert!(!edit(&mut path, 0, Action::Corner));
        assert_eq!(path, before);
    }
    #[test]
    fn coordinate_limit_failure_is_atomic() {
        let mut path = PathMask::default();
        path.commands[0] = PathCommand::Move {
            to: [32768.0, 100.0],
        };
        let before = path.clone();
        assert!(!edit(&mut path, 0, Action::Smooth));
        assert_eq!(path, before);
    }
}
