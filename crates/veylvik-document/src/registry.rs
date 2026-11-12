//! Authoritative ports, parameter bounds, defaults and JSON schemas for built-in nodes.
use serde::Serialize;
use serde_json::{Value, json};

pub const MAX_ASSET_NAME: usize = 4096;
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    Scalar,
    Seed,
    Color,
    Asset,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Parameter {
    pub name: &'static str,
    pub kind: ParameterKind,
    pub unit: &'static str,
    pub min: Option<f32>,
    pub max: Option<f32>,
    #[serde(skip_serializing)]
    pub default: f32,
}
impl Parameter {
    fn default_value(&self) -> Value {
        match self.kind {
            ParameterKind::Scalar => json!({"kind":"constant","value":self.default}),
            ParameterKind::Seed => json!(self.default as u32),
            ParameterKind::Color => json!([0.0, 0.0, 0.0, 1.0]),
            ParameterKind::Asset => json!("image.png"),
        }
    }
    fn schema(&self) -> Value {
        match self.kind {
            ParameterKind::Scalar => {
                let number = json!({"type":"number","minimum":self.min,"maximum":self.max});
                json!({"oneOf":[
                    {"type":"object","properties":{"kind":{"const":"constant"},"value":number},"required":["kind","value"],"additionalProperties":false},
                    {"type":"object","properties":{"kind":{"const":"linear"},"keys":{"type":"array","minItems":1,"items":{"type":"object","properties":{"time":{"type":"object","properties":{"ticks":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false},"value":number},"required":["time","value"],"additionalProperties":false}}},"required":["kind","keys"],"additionalProperties":false}
                ]})
            }
            ParameterKind::Seed => json!({"type":"integer","minimum":0,"maximum":u32::MAX}),
            ParameterKind::Color => {
                json!({"type":"array","minItems":4,"maxItems":4,"prefixItems":[{"type":"number"},{"type":"number"},{"type":"number"},{"type":"number","minimum":0,"maximum":1}]})
            }
            ParameterKind::Asset => {
                json!({"type":"string","minLength":1,"maxLength":MAX_ASSET_NAME})
            }
        }
    }
}
const fn scalar(
    name: &'static str,
    unit: &'static str,
    min: f32,
    max: f32,
    default: f32,
) -> Parameter {
    Parameter {
        name,
        kind: ParameterKind::Scalar,
        unit,
        min: Some(min),
        max: Some(max),
        default,
    }
}
const fn other(name: &'static str, kind: ParameterKind, unit: &'static str) -> Parameter {
    Parameter {
        name,
        kind,
        unit,
        min: None,
        max: None,
        default: 0.0,
    }
}
#[derive(Debug, Serialize)]
pub struct NodeType {
    pub type_id: &'static str,
    pub version: u32,
    pub label: &'static str,
    pub description: &'static str,
    pub inputs: &'static [&'static str],
    pub parameters: &'static [Parameter],
}
impl NodeType {
    pub fn default_operation(&self) -> Value {
        let mut value = json!({"type":self.type_id});
        for param in self.parameters {
            value[param.name] = param.default_value();
        }
        value
    }
    pub fn schema(&self) -> Value {
        let mut properties = json!({"type":{"const":self.type_id}});
        let mut required = vec!["type"];
        for param in self.parameters {
            properties[param.name] = param.schema();
            required.push(param.name);
        }
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
}
pub static SOLID: NodeType = NodeType {
    type_id: "solid",
    version: 1,
    label: "Solid",
    description: "Straight scene-linear Rec.709 colour, converted to premultiplied pixels.",
    inputs: &[],
    parameters: &[other("color", ParameterKind::Color, "linear RGBA")],
};
pub static NOISE: NodeType = NodeType {
    type_id: "noise",
    version: 1,
    label: "Noise",
    description: "Deterministic cell noise with a seed.",
    inputs: &[],
    parameters: &[
        other("seed", ParameterKind::Seed, "integer"),
        scalar("scale", "pixels", 1.0, 8192.0, 18.0),
    ],
};
pub static EXPOSURE: NodeType = NodeType {
    type_id: "exposure",
    version: 1,
    label: "Exposure",
    description: "Multiply RGB by two to the power of stops; preserve alpha.",
    inputs: &["image"],
    parameters: &[scalar("stops", "stops", -32.0, 32.0, 0.0)],
};
pub static TRANSFORM: NodeType = NodeType {
    type_id: "transform",
    version: 1,
    label: "Transform",
    description: "Bilinear translation with transparent borders.",
    inputs: &["image"],
    parameters: &[
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
    ],
};
pub static BLUR: NodeType = NodeType {
    type_id: "blur",
    version: 1,
    label: "Blur",
    description: "Box blur, radius rounded to an integer; transparent borders.",
    inputs: &["image"],
    parameters: &[scalar("radius", "pixels", 0.0, 32.0, 4.0)],
};
pub static MERGE: NodeType = NodeType {
    type_id: "merge",
    version: 1,
    label: "Merge",
    description: "Premultiplied foreground over background.",
    inputs: &["background", "foreground"],
    parameters: &[],
};
pub static IMAGE: NodeType = NodeType {
    type_id: "image",
    version: 1,
    label: "Image",
    description: "Host-supplied linear premultiplied image at native size, top-left anchored, cropped or padded to the composition.",
    inputs: &[],
    parameters: &[other("asset", ParameterKind::Asset, "asset key")],
};
pub static AFFINE: NodeType = NodeType {
    type_id: "affine",
    version: 1,
    label: "Affine transform",
    description: "Scale around a pixel-space pivot, rotate clockwise, then translate; bilinear sampling and transparent borders. Scale magnitude below 0.000001 produces transparent output.",
    inputs: &["image"],
    parameters: &[
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("scale_x", "factor", -1000.0, 1000.0, 1.0),
        scalar("scale_y", "factor", -1000.0, 1000.0, 1.0),
        scalar(
            "rotation",
            "degrees clockwise",
            -1_000_000.0,
            1_000_000.0,
            0.0,
        ),
        scalar("pivot_x", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("pivot_y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
    ],
};
pub static ELLIPSE: NodeType = NodeType {
    type_id: "ellipse",
    version: 1,
    label: "Ellipse mask",
    description: "White premultiplied ellipse coverage. Feather uses normalized radial distance scaled by the smaller radius, with at least a one-pixel transition. Zero radius is transparent.",
    inputs: &[],
    parameters: &[
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 320.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 180.0),
        scalar("radius_x", "pixels", 0.0, 8192.0, 100.0),
        scalar("radius_y", "pixels", 0.0, 8192.0, 100.0),
        scalar("feather", "pixels", 0.0, 8192.0, 1.0),
    ],
};
pub static MASK: NodeType = NodeType {
    type_id: "mask",
    version: 1,
    label: "Apply mask",
    description: "Multiply all premultiplied image channels by mask alpha; mask RGB is ignored.",
    inputs: &["image", "mask"],
    parameters: &[],
};
pub static TYPES: &[&NodeType] = &[
    &SOLID, &NOISE, &EXPOSURE, &TRANSFORM, &BLUR, &MERGE, &IMAGE, &AFFINE, &ELLIPSE, &MASK,
];
pub fn operation_schema() -> Value {
    json!({"oneOf":TYPES.iter().map(|node|node.schema()).collect::<Vec<_>>()})
}
pub fn catalog() -> Value {
    json!({"nodes":TYPES.iter().map(|node|json!({"descriptor":node,"default_operation":node.default_operation(),"operation_schema":node.schema()})).collect::<Vec<_>>(),"image_contract":"scene-linear Rec.709, premultiplied RGBA32F","scalar_contract":"constant or linear keyframes; time is ticks/rate seconds; keys strictly increase"})
}
