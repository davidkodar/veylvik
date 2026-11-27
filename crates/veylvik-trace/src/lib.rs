//! Path tracing for Veylvik's 3D scenes: each mesh gets a bounding volume hierarchy built once on the CPU, a small top level over the placed objects is built each frame, and both are walked on the GPU in compute shaders, so it runs on any Vulkan device with no ray-tracing hardware or experimental API.
pub mod bvh;
pub mod gpu;
pub mod scene;
pub use bvh::{Bvh, Hit, Node};
pub use gpu::{Environment, Job, Light, Settings, Tracer, View};
pub use scene::{CATCHER, Instance, MeshTree, Packed, Placed, Surface, UNLIT, pack};
