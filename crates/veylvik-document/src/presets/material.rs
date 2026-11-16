//! Materials as data: a saved material is one validated `Material` value, applied like a built-in.
use super::{Preset, builtin};
use crate::{Result, fail, nodes::Material};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;
pub const MAX_LABEL: usize = 80;

/// A user's material: what it is called and the full surface it applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub schema: u32,
    pub label: String,
    pub material: Material,
}
impl Saved {
    pub fn new(label: &str, material: &Material) -> Result<Self> {
        let saved = Self {
            schema: SCHEMA,
            label: label.trim().to_owned(),
            material: material.clone(),
        };
        saved.validate()?;
        Ok(saved)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            return fail(format!(
                "Unsupported material version {} (this Veylvik reads version {SCHEMA})",
                self.schema
            ));
        }
        let length = self.label.chars().count();
        if !(1..=MAX_LABEL).contains(&length) || self.label.chars().any(char::is_control) {
            return fail(format!(
                "A material name needs 1–{MAX_LABEL} characters without control characters"
            ));
        }
        // The same checks as a project: registered bounds and the node's own.
        let mut doc = crate::blank(16, 16, crate::Time::new(24, 1)?)?;
        let node = crate::Node::new(
            &self.label,
            crate::Operation::Material(self.material.clone()),
            &[("image", doc.output)],
        );
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
        crate::Document::new(doc).map(drop)
    }
}

/// Where a material comes from: a built-in library id or a saved material.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    Builtin(&'a str),
    Saved(&'a Saved),
}
impl<'a> From<&'a str> for Source<'a> {
    fn from(id: &'a str) -> Self {
        Self::Builtin(id)
    }
}
impl<'a> From<&'a Saved> for Source<'a> {
    fn from(saved: &'a Saved) -> Self {
        Self::Saved(saved)
    }
}
impl Source<'_> {
    pub fn preset(self) -> Result<Preset> {
        match self {
            Self::Builtin(id) if builtin::is_material(id) => builtin::build(id),
            Self::Builtin(_) => fail("This effect is not a full-frame material"),
            Self::Saved(saved) => {
                saved.validate()?;
                builtin::material_preset(&saved.label, &saved.material)
            }
        }
    }
    /// The surface a built-in or saved material applies to existing text.
    pub fn material(self) -> Result<Material> {
        match self {
            Self::Saved(saved) => Ok(saved.material.clone()),
            Self::Builtin(_) => self
                .preset()?
                .nodes
                .into_values()
                .find_map(|n| match n.operation {
                    crate::Operation::Material(m) => Some(m),
                    _ => None,
                })
                .ok_or_else(|| crate::Error("The preset has no material".into())),
        }
    }
}

/// Restyle an existing material with another one. A family built-in changes only
/// the surface family, so the user's finish and lighting stay; a catalogue look or a
/// saved material is a whole look and brings its finish, optics and lighting rig. Size,
/// bevel and reflection motion belong to the layer and always stay.
pub fn restyle(target: &mut Material, source: Source<'_>, with: &Material) {
    target.kind = with.kind;
    target.optics = with.optics;
    target.colours = with.colours.clone();
    let whole_look = match source {
        Source::Saved(_) => true,
        Source::Builtin(id) => super::catalogue::get(id).is_some(),
    };
    if whole_look {
        target.smooth_edges = with.smooth_edges;
        target.brushing = with.brushing;
        target.studio = with.studio.clone();
        target.roughness = with.roughness.clone();
        target.grain = with.grain.clone();
        target.emission = with.emission.clone();
    }
}

/// Library groups for built-in materials.
pub fn category(id: &str) -> &'static str {
    if let Some(entry) = super::catalogue::get(id) {
        return entry.category;
    }
    if matches!(
        id,
        "gold_text" | "silver_text" | "aluminium_text" | "bronze_text"
    ) {
        "Metal"
    } else if id.starts_with("leather") {
        "Leather"
    } else {
        "Natural"
    }
}

/// Put a built-in or saved material on a 3D shape or 3D text as its look: an
/// existing look is restyled (so a family keeps its finish), a plain surface gets
/// the material whole. False for other operations.
pub fn apply_to_object(
    operation: &mut crate::Operation,
    source: Source<'_>,
    with: &Material,
) -> bool {
    let look = match operation {
        crate::Operation::Shape3d(s) => &mut s.look,
        crate::Operation::Text3d(t) => &mut t.look,
        _ => return false,
    };
    match look {
        Some(current) => restyle(current, source, with),
        None => *look = Some(Box::new(with.clone())),
    }
    true
}

/// Commands that put a material on `target`, as the library does: a Text node gets
/// (or restyles) its Text surface; a Text surface is restyled in place; a 3D shape
/// or 3D text gets it as its look. One transaction; output and wiring stay.
pub fn apply_commands(
    doc: &crate::Composition,
    target: crate::NodeId,
    source: Source<'_>,
) -> Result<Vec<crate::Command>> {
    let node = doc
        .nodes
        .get(&target)
        .ok_or_else(|| crate::Error("the target node is missing".into()))?;
    let mut node = node.clone();
    match &mut node.operation {
        crate::Operation::Text(_) => {
            return super::text_style::apply(doc, target, Some(source), None);
        }
        crate::Operation::Material(current) => {
            let with = source.material()?;
            restyle(current, source, &with);
        }
        operation => {
            let with = source.material()?;
            if !apply_to_object(operation, source, &with) {
                return fail("Materials apply to text, a Text surface, 3D text or a 3D shape");
            }
        }
    }
    Ok(vec![crate::Command::Put {
        node: Box::new(node),
    }])
}
