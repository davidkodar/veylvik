//! SVG import: every filled or stroked shape of a drawing becomes closed path geometry in composition pixels with one flat colour, ready to be Path nodes. Text, pictures, clips and masks are not read; a gradient gives the average of its stops.
use crate::Result;
use serde_json::{Value, json};
use std::path::Path;
use veylvik_document::{
    Command, Composition, Node, NodeId, NodePosition, Operation,
    vector::{FillRule, MAX_PATH_COMMANDS, PathCommand, PathMask},
};

/// The most shapes one drawing may bring in.
pub const MAX_SHAPES: usize = 64;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Whether `path` names an SVG drawing.
pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}
/// One shape of the drawing.
#[derive(Debug, Clone)]
pub struct Shape {
    pub label: String,
    /// Straight scene-linear RGBA.
    pub colour: [f32; 4],
    pub geometry: PathMask,
}
fn linear(srgb: u8) -> f32 {
    let v = f32::from(srgb) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
/// The paint as one colour: its own, or the average of a gradient's stops.
fn colour(paint: &usvg::Paint, opacity: f32) -> Option<[f32; 4]> {
    let stops = |stops: &[usvg::Stop]| {
        let n = stops.len().max(1) as f32;
        let mut sum = [0.0; 4];
        for stop in stops {
            let c = stop.color();
            for (total, value) in sum.iter_mut().zip([
                linear(c.red),
                linear(c.green),
                linear(c.blue),
                stop.opacity().get(),
            ]) {
                *total += value / n;
            }
        }
        sum[3] *= opacity;
        sum
    };
    match paint {
        usvg::Paint::Color(c) => Some([linear(c.red), linear(c.green), linear(c.blue), opacity]),
        usvg::Paint::LinearGradient(g) => Some(stops(g.stops())),
        usvg::Paint::RadialGradient(g) => Some(stops(g.stops())),
        usvg::Paint::Pattern(_) => None,
    }
}
/// `path` as closed contours: a fill closes every contour, as SVG does.
fn geometry(path: &usvg::tiny_skia_path::Path, rule: FillRule) -> Option<PathMask> {
    use usvg::tiny_skia_path::PathSegment as S;
    let mut commands = Vec::new();
    let (mut open, mut segments) = (false, 0);
    let close = |commands: &mut Vec<PathCommand>, open: &mut bool, segments: &mut usize| {
        if *open {
            if *segments > 0 {
                commands.push(PathCommand::Close {});
            } else {
                commands.pop();
            }
        }
        (*open, *segments) = (false, 0);
    };
    let at = |p: usvg::tiny_skia_path::Point| [p.x, p.y];
    for segment in path.segments() {
        match segment {
            S::MoveTo(p) => {
                close(&mut commands, &mut open, &mut segments);
                commands.push(PathCommand::Move { to: at(p) });
                open = true;
            }
            S::Close => close(&mut commands, &mut open, &mut segments),
            _ if !open => {}
            S::LineTo(p) => {
                commands.push(PathCommand::Line { to: at(p) });
                segments += 1;
            }
            S::QuadTo(c, p) => {
                commands.push(PathCommand::Quad {
                    control: at(c),
                    to: at(p),
                });
                segments += 1;
            }
            S::CubicTo(a, b, p) => {
                commands.push(PathCommand::Cubic {
                    control1: at(a),
                    control2: at(b),
                    to: at(p),
                });
                segments += 1;
            }
        }
    }
    close(&mut commands, &mut open, &mut segments);
    let mask = PathMask {
        keys: vec![],
        fill_rule: rule,
        commands,
    };
    mask.validate().is_ok().then_some(mask)
}
fn walk(
    group: &usvg::Group,
    fit: usvg::Transform,
    shapes: &mut Vec<Shape>,
    skipped: &mut usize,
) -> Result<()> {
    for node in group.children() {
        match node {
            usvg::Node::Group(inner) => walk(inner, fit, shapes, skipped)?,
            usvg::Node::Path(path) if path.is_visible() => {
                let placed = fit.pre_concat(path.abs_transform());
                let base = if path.id().is_empty() {
                    format!("Shape {}", shapes.len() + 1)
                } else {
                    path.id().to_string()
                };
                let name = |part: &str| format!("{base} {part}");
                let mut add = |label: String,
                               paint: &usvg::Paint,
                               opacity: f32,
                               outline: Option<usvg::tiny_skia_path::Path>,
                               rule: FillRule|
                 -> Result<()> {
                    let made = outline
                        .and_then(|o| o.transform(placed))
                        .and_then(|o| geometry(&o, rule))
                        .zip(colour(paint, opacity));
                    let Some((geometry, colour)) = made else {
                        *skipped += 1;
                        return Ok(());
                    };
                    if shapes.len() >= MAX_SHAPES {
                        return Err(format!(
                            "the drawing has more than {MAX_SHAPES} shapes: simplify or merge it first"
                        )
                        .into());
                    }
                    shapes.push(Shape {
                        label,
                        colour,
                        geometry,
                    });
                    Ok(())
                };
                if let Some(fill) = path.fill() {
                    let rule = match fill.rule() {
                        usvg::FillRule::NonZero => FillRule::NonZero,
                        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                    };
                    add(
                        name("fill"),
                        fill.paint(),
                        fill.opacity().get(),
                        Some(path.data().clone()),
                        rule,
                    )?;
                }
                if let Some(stroke) = path.stroke() {
                    // The outline of the line, at four times the drawing's detail.
                    let outline = path.data().stroke(&stroke.to_tiny_skia(), 4.0);
                    add(
                        name("line"),
                        stroke.paint(),
                        stroke.opacity().get(),
                        outline,
                        FillRule::NonZero,
                    )?;
                }
            }
            // Hidden paths, text and pictures are not shapes.
            _ => *skipped += 1,
        }
    }
    Ok(())
}
/// The drawing at `path` as shapes, back to front, fitted inside `scale` (0.05 to 1)
/// of a `width` x `height` frame and centred, with the number of parts left out
/// (text, pictures, patterns, empty or oversized paths).
pub fn import(path: &Path, width: u32, height: u32, scale: f32) -> Result<(Vec<Shape>, usize)> {
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Err("the SVG file is larger than 16 MiB".into());
    }
    let tree = usvg::Tree::from_data(&std::fs::read(path)?, &usvg::Options::default())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = (tree.size().width(), tree.size().height());
    let fit = scale.clamp(0.05, 1.0) * (width as f32 / w).min(height as f32 / h);
    let fit = usvg::Transform::from_row(
        fit,
        0.0,
        0.0,
        fit,
        (width as f32 - w * fit) * 0.5,
        (height as f32 - h * fit) * 0.5,
    );
    let (mut shapes, mut skipped) = (Vec::new(), 0);
    walk(tree.root(), fit, &mut shapes, &mut skipped)?;
    if shapes.is_empty() {
        return Err("the SVG has no filled or stroked shapes to import".into());
    }
    if shapes
        .iter()
        .map(|s| s.geometry.commands.len())
        .sum::<usize>()
        > 8 * MAX_PATH_COMMANDS
    {
        return Err("the drawing is too detailed: simplify it first".into());
    }
    Ok((shapes, skipped))
}
/// The nodes that show `shapes`: for each a Path (its outline, the node to extrude
/// with 3D text), a Solid of its colour and a Mask joining them, merged back to
/// front. Returns the commands and a report with every node's id.
pub fn commands(composition: &Composition, name: &str, shapes: &[Shape]) -> (Vec<Command>, Value) {
    let top = composition
        .nodes
        .values()
        .filter_map(|n| n.position.map(|p| p.y))
        .fold(0.0, f32::max)
        + 220.0;
    let mut nodes: Vec<Node> = Vec::new();
    let mut add = |mut node: Node, column: usize, row: usize| -> NodeId {
        node.position = Some(NodePosition {
            x: column as f32 * 230.0,
            y: top + row as f32 * 145.0,
        });
        let id = node.id;
        nodes.push(node);
        id
    };
    let mut report = Vec::new();
    let mut under: Option<NodeId> = None;
    for (i, shape) in shapes.iter().enumerate() {
        let outline = add(
            Node::new(
                &shape.label,
                Operation::Path(veylvik_document::nodes::Path {
                    geometry: shape.geometry.clone(),
                }),
                &[],
            ),
            0,
            i,
        );
        let solid = add(
            Node::new(
                &format!("{} colour", shape.label),
                Operation::Solid(veylvik_document::nodes::Solid {
                    color: shape.colour,
                }),
                &[],
            ),
            1,
            i,
        );
        let mut layer = add(
            Node::new(
                &format!("{} layer", shape.label),
                Operation::Mask(veylvik_document::nodes::Mask {}),
                &[("image", solid), ("mask", outline)],
            ),
            2,
            i,
        );
        report.push(json!({"label":shape.label,"outline":outline,"colour_node":solid,"layer":layer,"colour":shape.colour}));
        if let Some(below) = under {
            layer = add(
                Node::new(
                    &format!("{name} {}", i + 1),
                    Operation::Merge(veylvik_document::nodes::Merge {}),
                    &[("background", below), ("foreground", layer)],
                ),
                3,
                i,
            );
        }
        under = Some(layer);
    }
    let result = under.expect("at least one shape");
    let commands = nodes
        .into_iter()
        .map(|node| Command::Put {
            node: Box::new(node),
        })
        .collect();
    (commands, json!({"result":result,"shapes":report}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_and_lines_become_closed_coloured_shapes_fitted_to_the_frame() {
        let dir = std::env::temp_dir().join(format!("veylvik-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mark.svg");
        std::fs::write(
            &file,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">
              <rect id="tile" fill="#ff0000" width="100" height="50"/>
              <g transform="translate(10 0)"><path id="v" fill="none" stroke="#ffffff" stroke-width="4" d="M10 10 L30 40 L50 10"/></g>
              <text x="5" y="5">no</text>
            </svg>"##,
        )
        .unwrap();
        let (shapes, _) = import(&file, 1000, 1000, 1.0).unwrap();
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0].label, "tile fill");
        assert_eq!(shapes[0].colour, [1.0, 0.0, 0.0, 1.0]);
        // 100 x 50 fitted into 1000 x 1000: ten times larger, centred vertically.
        let bounds = |shape: &Shape| {
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for command in &shape.geometry.commands {
                if let PathCommand::Move { to } | PathCommand::Line { to } = command {
                    b = [
                        b[0].min(to[0]),
                        b[1].min(to[1]),
                        b[2].max(to[0]),
                        b[3].max(to[1]),
                    ];
                }
            }
            b
        };
        assert_eq!(bounds(&shapes[0]), [0.0, 250.0, 1000.0, 750.0]);
        // The open line became an outline about 40 pixels wide, moved by its group.
        assert_eq!(shapes[1].label, "v line");
        let line = bounds(&shapes[1]);
        assert!(
            line[0] > 150.0 && line[0] < 200.0 && line[2] > 600.0 && line[2] < 650.0,
            "{line:?}"
        );
        shapes[1].geometry.validate().unwrap();
        let doc =
            veylvik_document::blank(1000, 1000, veylvik_document::Time { ticks: 24, rate: 1 })
                .unwrap();
        let (commands, report) = commands(&doc, "Mark", &shapes);
        let mut document = veylvik_document::Document::new(doc).unwrap();
        document.apply(0, &commands).unwrap();
        assert_eq!(report["shapes"].as_array().unwrap().len(), 2);
        assert!(import(&dir.join("missing.svg"), 10, 10, 1.0).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
