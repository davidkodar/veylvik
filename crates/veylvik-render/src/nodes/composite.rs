//! GPU passes and uniforms for layering, blending and mask nodes.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Merge {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Merge)
    }
    fn shareable(&self) -> bool {
        true
    }
}
impl RenderNode for veylvik_document::nodes::Blend {
    /// At zero opacity the result is exactly the background.
    fn idle_inputs(&self, sample: &dyn Fn(&str) -> Result<f32>) -> Result<&'static [&'static str]> {
        Ok(if sample("opacity")? == 0.0 {
            &["foreground"]
        } else {
            &[]
        })
    }
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Blend)
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
        let veylvik_document::nodes::Blend { mode, .. } = self;
        {
            p.size_op[3] = *mode as u32;
            p.values[0] = sample("opacity")?;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Mask {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Mask)
    }
    fn shareable(&self) -> bool {
        true
    }
}
impl RenderNode for veylvik_document::nodes::CombineMask {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::CombineMask)
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
        let veylvik_document::nodes::CombineMask { mode } = self;
        {
            p.size_op[3] = *mode as u32;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::ChannelMask {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::ChannelMask)
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
        let veylvik_document::nodes::ChannelMask { channel } = self;
        {
            p.size_op[3] = *channel as u32;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::SurfaceBlend {
    /// At zero opacity the result is exactly the background.
    fn idle_inputs(&self, sample: &dyn Fn(&str) -> Result<f32>) -> Result<&'static [&'static str]> {
        Ok(if sample("opacity")? == 0.0 {
            &["foreground"]
        } else {
            &[]
        })
    }
    fn passes(&self) -> Passes {
        Passes::One(Kernel::SurfaceBlend)
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
                sample("opacity")?,
                sample("lighting")?,
                sample("detail")?,
                sample("wear")?,
            ];
            p.extra = [
                sample("threshold")?,
                sample("distortion")?,
                sample("softness")?,
                sample("grain")?,
            ];
            // Grain is deterministic at a given render time, including repeated previews.
            if p.extra[3] > 0. {
                let ms = (time.seconds()? * 1000.).round() as i64;
                p.size_op[3] = (ms as u64 ^ (ms as u64 >> 32)) as u32;
            }
        }
        Ok(())
    }
}
