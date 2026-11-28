//! One struct per node type, grouped by family; `Operation` wraps them by type id.

use crate::{Result, Scalar, registry::NodeType};

/// Behaviour each node type carries with its data, so core code dispatches instead of
/// matching on every type.
pub trait NodeOp {
    /// Type id, ports and registered parameters with their bounds.
    fn descriptor(&self) -> &'static NodeType;
    /// The animatable parameter registered under `name`.
    fn scalar(&self, name: &str) -> Option<&Scalar>;
    fn scalar_mut(&mut self, name: &str) -> Option<&mut Scalar>;
    /// Checks beyond the registered scalar bounds, which apply to every node.
    fn validate(&self) -> Result<()> {
        Ok(())
    }
    /// Project asset key, for nodes that read media files.
    fn asset(&self) -> Option<&str> {
        None
    }
    fn asset_mut(&mut self) -> Option<&mut String> {
        None
    }
}
/// Implements `NodeOp` from the descriptor and the struct's `Scalar` fields; an optional
/// block adds the node's own validation or asset access.
macro_rules! node {
    ($ty:ident, $descriptor:ident, [$($field:ident),*] $(, { $($extra:tt)* })?) => {
        impl super::NodeOp for $ty {
            fn descriptor(&self) -> &'static crate::registry::NodeType {
                &$descriptor
            }
            #[allow(unused_variables)]
            fn scalar(&self, name: &str) -> Option<&crate::Scalar> {
                match name {
                    $(stringify!($field) => Some(&self.$field),)*
                    _ => None,
                }
            }
            #[allow(unused_variables)]
            fn scalar_mut(&mut self, name: &str) -> Option<&mut crate::Scalar> {
                match name {
                    $(stringify!($field) => Some(&mut self.$field),)*
                    _ => None,
                }
            }
            $($($extra)*)?
        }
    };
}
mod camera_orbit;
pub use camera_orbit::CameraOrbit;
mod color;
mod composite;
mod custom;
mod figure;
mod filters;
mod flutter;
mod forces;
mod generators;
mod instances;
mod keying;
mod lights;
mod marker;
mod materials;
mod media;
mod ofx;
mod particles;
mod projection;
mod scene;
mod sized;
mod sound;
mod splats;
mod text;
mod tracking;
mod vector;
mod volume;
mod warp;
pub use color::*;
pub use composite::*;
pub use custom::*;
pub use figure::*;
pub use filters::*;
pub use flutter::*;
pub use forces::*;
pub use generators::*;
pub use instances::*;
pub use keying::*;
pub use lights::*;
pub use marker::*;
pub use materials::*;
pub use media::*;
pub use ofx::*;
pub use particles::*;
pub use projection::*;
pub use scene::*;
pub use sized::*;
pub use sound::*;
pub use splats::*;
pub use text::*;
pub use tracking::*;
pub use vector::*;
pub use volume::*;
pub use warp::*;
