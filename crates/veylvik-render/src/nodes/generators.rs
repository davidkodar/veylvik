//! GPU passes and uniforms for generator nodes.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Solid {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Solid)
    }
    fn shareable(&self) -> bool {
        true
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        let veylvik_document::nodes::Solid { color } = self;
        {
            p.values = *color;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Gradient {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Gradient)
    }
    fn shareable(&self) -> bool {
        true
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        let veylvik_document::nodes::Gradient { mode, .. } = self;
        {
            p.size_op[3] = *mode as u32;
            p.values = [
                sample("start_x")?,
                sample("start_y")?,
                sample("end_x")?,
                sample("end_y")?,
            ];
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Noise {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Noise)
    }
    fn shareable(&self) -> bool {
        true
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        let veylvik_document::nodes::Noise { seed, .. } = self;
        {
            p.size_op[3] = *seed;
            p.values[0] = sample("scale")?;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::FractalNoise {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::FractalNoise)
    }
    fn shareable(&self) -> bool {
        true
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        let veylvik_document::nodes::FractalNoise { seed, .. } = self;
        {
            p.size_op[3] = *seed;
            p.values = [
                sample("scale")?,
                sample("octaves")?.round(),
                sample("roughness")?,
                sample("evolution")?,
            ];
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Fire {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Fire)
    }
    fn shareable(&self) -> bool {
        true
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        let veylvik_document::nodes::Fire { seed, .. } = self;
        {
            p.size_op[3] = *seed;
            p.values = [
                sample("x")?,
                sample("y")?,
                sample("width")?,
                sample("height")?,
            ];
            p.extra = [
                sample("evolution")?.rem_euclid(4.0),
                sample("intensity")?,
                sample("turbulence")?,
                0.0,
            ];
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Atmosphere {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Atmosphere)
    }
    fn shareable(&self) -> bool {
        true
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        let veylvik_document::nodes::Atmosphere { kind, seed, .. } = self;
        {
            p.size_op[2] = *seed;
            p.size_op[3] = *kind as u32;
            p.values = [
                sample("x")?,
                sample("y")?,
                sample("width")?,
                sample("height")?,
            ];
            p.extra = [
                sample("evolution")?.rem_euclid(4.0),
                sample("density")?,
                sample("detail")?,
                sample("lighting")?,
            ];
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Ellipse {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Ellipse)
    }
    fn shareable(&self) -> bool {
        true
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        {
            p.values = [
                sample("x")?,
                sample("y")?,
                sample("radius_x")?,
                sample("radius_y")?,
            ];
            p.extra[0] = sample("feather")?.max(1.0);
        }
        Ok(())
    }
}
