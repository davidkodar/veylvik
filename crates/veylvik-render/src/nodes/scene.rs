//! 3D scene nodes carry data on Scene ports and have no image pass; Render scene is the bridge back to images.
use super::RenderNode;
use veylvik_document::nodes::*;

impl RenderNode for Shape3d {}
impl RenderNode for Text3d {}
impl RenderNode for ImageCard {}
/// Its input is planned as a branch of its own size; its passes place that picture in
/// the frame where a frame-sized image is needed.
impl RenderNode for OwnSize {
    fn passes(&self) -> super::Passes {
        super::Passes::One(crate::Kernel::OwnSizeFit)
    }
    fn params(
        &self,
        _cx: &super::Cx<'_>,
        _sample: &dyn Fn(&str) -> crate::Result<f32>,
        p: &mut crate::Params,
    ) -> crate::Result<()> {
        let fit = Fit::ALL.iter().position(|f| *f == self.fit).unwrap_or(1);
        p.values = [self.width as f32, self.height as f32, fit as f32, 0.0];
        Ok(())
    }
}
impl RenderNode for Transform3d {}
impl RenderNode for MergeScenes {}
impl RenderNode for Camera3d {}
impl RenderNode for Light3d {}
impl RenderNode for RenderScene {}
impl RenderNode for Environment3d {}
impl RenderNode for PointCloud {}
impl RenderNode for Model3d {}
/// Read by Render scene, which draws its objects at the particles' places.
impl RenderNode for ParticleInstances {}
/// Read by Render scene, which colours its objects through the camera.
impl RenderNode for CameraProjection {}
/// Read by Render scene, which draws its splats.
impl RenderNode for GaussianSplats {}
/// Read by Render scene, which draws its body.
impl RenderNode for Figure3d {}
impl RenderNode for Marker3d {}
