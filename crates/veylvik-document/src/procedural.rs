//! A portable procedural motion study composed entirely from built-in nodes.
use crate::{
    BlendMode, Composition, Key, MaskChannel, Node, NodeId, Operation, Scalar, Time,
    vector::{FillRule, PathCommand, PathMask},
};
struct Scene {
    nodes: Vec<Node>,
}
impl Scene {
    fn add(&mut self, label: &str, operation: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
        let node = Node::new(label, operation, inputs);
        let id = node.id;
        self.nodes.push(node);
        id
    }
    fn over(&mut self, background: NodeId, foreground: NodeId, label: &str) -> NodeId {
        self.add(
            label,
            Operation::Blend {
                mode: BlendMode::Add,
                opacity: 1.0.into(),
            },
            &[("background", background), ("foreground", foreground)],
        )
    }
    fn color(&mut self, label: &str, color: [f32; 4], mask: NodeId) -> NodeId {
        let solid = self.add(&format!("{label} color"), Operation::Solid { color }, &[]);
        self.add(
            label,
            Operation::Mask {},
            &[("image", solid), ("mask", mask)],
        )
    }
    fn halo(&mut self, image: NodeId, radius: f32, stops: f32, label: &str) -> NodeId {
        let mut blur = image;
        for pass in 1..=3 {
            blur = self.add(
                &format!("{label} blur {pass}"),
                Operation::Blur {
                    radius: (radius * 0.6).into(),
                },
                &[("image", blur)],
            );
        }
        self.add(
            label,
            Operation::Exposure {
                stops: stops.into(),
            },
            &[("image", blur)],
        )
    }
}
fn animated(start: f32, end: f32) -> Scalar {
    Scalar::Linear {
        keys: vec![
            Key {
                segment: None,
                time: Time { ticks: 0, rate: 1 },
                value: start,
            },
            Key {
                segment: None,
                time: Time { ticks: 6, rate: 1 },
                value: end,
            },
        ],
    }
}
fn circle(commands: &mut Vec<PathCommand>, x: f32, y: f32, r: f32) {
    let k = r * 0.552_284_8;
    commands.extend([
        PathCommand::Move { to: [x + r, y] },
        PathCommand::Cubic {
            control1: [x + r, y + k],
            control2: [x + k, y + r],
            to: [x, y + r],
        },
        PathCommand::Cubic {
            control1: [x - k, y + r],
            control2: [x - r, y + k],
            to: [x - r, y],
        },
        PathCommand::Cubic {
            control1: [x - r, y - k],
            control2: [x - k, y - r],
            to: [x, y - r],
        },
        PathCommand::Cubic {
            control1: [x + k, y - r],
            control2: [x + r, y - k],
            to: [x + r, y],
        },
        PathCommand::Close {},
    ]);
}
/// Six seconds of evolving energy rings, colored glow, clouds and orbiting sparks.
/// No fonts, images, or other external assets are required.
pub fn procedural_demo() -> Composition {
    let mut scene = Scene { nodes: Vec::new() };
    let mut output = scene.add(
        "Deep space",
        Operation::Solid {
            color: [0.002, 0.004, 0.012, 1.0],
        },
        &[],
    );
    let turbulence = scene.add(
        "Evolving turbulence",
        Operation::FractalNoise {
            seed: 73,
            scale: 90.0.into(),
            octaves: 5.0.into(),
            roughness: 0.55.into(),
            evolution: animated(0.0, 1.8),
        },
        &[],
    );
    let texture = scene.add(
        "Cloud coverage",
        Operation::ChannelMask {
            channel: MaskChannel::Luminance,
        },
        &[("image", turbulence)],
    );
    let core = scene.add(
        "Core silhouette",
        Operation::Ellipse {
            x: 480.0.into(),
            y: 270.0.into(),
            radius_x: 150.0.into(),
            radius_y: 120.0.into(),
            feather: 110.0.into(),
        },
        &[],
    );
    let core = scene.add(
        "Textured core",
        Operation::Mask {},
        &[("image", core), ("mask", texture)],
    );
    let core = scene.color("Violet atmosphere", [0.28, 0.025, 0.95, 1.0], core);
    let core = scene.halo(core, 8.0, 0.0, "Soft atmosphere");
    output = scene.over(output, core, "Atmosphere composite");
    for (index, (radius, thickness, color, x, y, scale_y, angle)) in [
        (166.0, 3.5, [0.015, 2.2, 4.0, 1.0], 38.0, 26.0, 1.0, 0.0),
        (
            218.0,
            2.5,
            [4.0, 0.25, 0.025, 1.0],
            -24.0,
            18.0,
            0.55,
            -28.0,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let name = if index == 0 {
            "Ion ring"
        } else {
            "Solar orbit"
        };
        let mut commands = Vec::new();
        circle(&mut commands, 480.0, 270.0, radius);
        circle(&mut commands, 480.0, 270.0, radius - thickness);
        let mask = scene.add(
            &format!("{name} outline"),
            Operation::Path {
                geometry: PathMask {
                    fill_rule: FillRule::EvenOdd,
                    commands,
                },
            },
            &[],
        );
        let mask = scene.add(
            &format!("{name} tilt"),
            Operation::Affine {
                x: 0.0.into(),
                y: 0.0.into(),
                scale_x: 1.0.into(),
                scale_y: scale_y.into(),
                rotation: animated(angle, angle + 24.0),
                pivot_x: 480.0.into(),
                pivot_y: 270.0.into(),
            },
            &[("image", mask)],
        );
        let mask = scene.add(
            &format!("{name} distortion"),
            Operation::Displace {
                x: x.into(),
                y: y.into(),
            },
            &[("image", mask), ("map", turbulence)],
        );
        let ring = scene.color(name, color, mask);
        let halo = scene.halo(ring, 14.0, 1.2, &format!("{name} glow"));
        output = scene.over(output, halo, &format!("{name} halo composite"));
        output = scene.over(output, ring, &format!("{name} detail composite"));
    }
    for (index, (x, y, radius, phase)) in [
        (660.0, 270.0, 3.0, 0.0),
        (660.0, 270.0, 2.0, 120.0),
        (660.0, 270.0, 2.5, 240.0),
    ]
    .into_iter()
    .enumerate()
    {
        let label = format!("Spark {}", index + 1);
        let mask = scene.add(
            &format!("{label} mask"),
            Operation::Ellipse {
                x: x.into(),
                y: y.into(),
                radius_x: radius.into(),
                radius_y: radius.into(),
                feather: 1.0.into(),
            },
            &[],
        );
        let mask = scene.add(
            &format!("{label} orbit"),
            Operation::Affine {
                x: 0.0.into(),
                y: 0.0.into(),
                scale_x: 1.0.into(),
                scale_y: 0.8.into(),
                rotation: animated(phase, phase + 360.0),
                pivot_x: 480.0.into(),
                pivot_y: 270.0.into(),
            },
            &[("image", mask)],
        );
        let spark = scene.color(&label, [1.2, 2.2, 4.0, 1.0], mask);
        let halo = scene.halo(spark, 6.0, 1.0, &format!("{label} halo"));
        output = scene.over(output, halo, &format!("{label} glow composite"));
        output = scene.over(output, spark, &format!("{label} composite"));
    }
    output = scene.add(
        "Output",
        Operation::Exposure {
            stops: (-0.3).into(),
        },
        &[("image", output)],
    );
    Composition {
        groups: Default::default(),
        schema: 1,
        frame_count: 144,
        width: 960,
        height: 540,
        frame_rate: Time { ticks: 24, rate: 1 },
        output,
        nodes: scene
            .nodes
            .into_iter()
            .map(|node| (node.id, node))
            .collect(),
    }
}
