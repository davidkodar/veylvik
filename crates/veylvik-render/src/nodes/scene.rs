//! 3D scene nodes carry data on Scene ports and have no image pass; Render scene is the bridge back to images.
use super::RenderNode;
use veylvik_document::nodes::*;

impl RenderNode for Shape3d {}
impl RenderNode for Text3d {}
impl RenderNode for ImageCard {}
impl RenderNode for Transform3d {}
impl RenderNode for MergeScenes {}
impl RenderNode for Camera3d {}
impl RenderNode for Light3d {}
impl RenderNode for RenderScene {}
impl RenderNode for Environment3d {}
impl RenderNode for PointCloud {}
impl RenderNode for Model3d {}
