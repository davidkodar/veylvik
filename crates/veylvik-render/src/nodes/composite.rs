//! GPU passes and uniforms for layering, blending and mask nodes.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Merge {
    // Each pixel is made of the two pixels at its own place.
    fn reach(&self, _: &dyn Fn(&str) -> Result<f32>, _: &str) -> Result<crate::nodes::Reach> {
        Ok(crate::nodes::Reach::Same)
    }
    fn extent(
        &self,
        _: &dyn Fn(&str) -> Result<f32>,
        input: &dyn Fn(&str) -> crate::nodes::Extent,
    ) -> Result<crate::nodes::Extent> {
        Ok(input("background").union(input("foreground")))
    }
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Merge)
    }
    fn shareable(&self) -> bool {
        true
    }
}
impl RenderNode for veylvik_document::nodes::Blend {
    fn reach(&self, _: &dyn Fn(&str) -> Result<f32>, _: &str) -> Result<crate::nodes::Reach> {
        Ok(crate::nodes::Reach::Same)
    }
    fn extent(
        &self,
        _: &dyn Fn(&str) -> Result<f32>,
        input: &dyn Fn(&str) -> crate::nodes::Extent,
    ) -> Result<crate::nodes::Extent> {
        Ok(input("background").union(input("foreground")))
    }
    /// At zero opacity the result is exactly the background.
    fn idle_inputs(
        &self,
        sample: &dyn Fn(&str) -> Result<f32>,
        _time: veylvik_document::Time,
    ) -> Result<&'static [&'static str]> {
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
            ..
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
    // The image, kept or let through pixel by pixel: never more than the image.
    fn reach(&self, _: &dyn Fn(&str) -> Result<f32>, _: &str) -> Result<crate::nodes::Reach> {
        Ok(crate::nodes::Reach::Same)
    }
    /// The image times the mask's coverage: clear where either is.
    fn extent(
        &self,
        _: &dyn Fn(&str) -> Result<f32>,
        input: &dyn Fn(&str) -> crate::nodes::Extent,
    ) -> Result<crate::nodes::Extent> {
        Ok(input("image").intersect(input("mask")))
    }
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Mask)
    }
    fn shareable(&self) -> bool {
        true
    }
}
impl RenderNode for veylvik_document::nodes::CombineMask {
    // The two coverages at the same place.
    fn reach(&self, _: &dyn Fn(&str) -> Result<f32>, _: &str) -> Result<crate::nodes::Reach> {
        Ok(crate::nodes::Reach::Same)
    }
    /// Added or either but not both: where either is; overlap: where both are; the
    /// first less the other: where the first is.
    fn extent(
        &self,
        _: &dyn Fn(&str) -> Result<f32>,
        input: &dyn Fn(&str) -> crate::nodes::Extent,
    ) -> Result<crate::nodes::Extent> {
        let (a, b) = (input("mask"), input("other"));
        Ok(match self.mode as u32 {
            1 => a.intersect(b),
            2 => a,
            _ => a.union(b),
        })
    }
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
            ..
        } = *cx;
        let veylvik_document::nodes::CombineMask { mode } = self;
        {
            p.size_op[3] = *mode as u32;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::ChannelMask {
    // Coverage read from the pixel at the same place.
    fn reach(&self, _: &dyn Fn(&str) -> Result<f32>, _: &str) -> Result<crate::nodes::Reach> {
        Ok(crate::nodes::Reach::Same)
    }
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
            ..
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
    fn idle_inputs(
        &self,
        sample: &dyn Fn(&str) -> Result<f32>,
        _time: veylvik_document::Time,
    ) -> Result<&'static [&'static str]> {
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
            ..
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
impl RenderNode for veylvik_document::nodes::Fade {
    fn reach(&self, _: &dyn Fn(&str) -> Result<f32>, _: &str) -> Result<crate::nodes::Reach> {
        Ok(crate::nodes::Reach::Same)
    }
    fn extent(
        &self,
        _: &dyn Fn(&str) -> Result<f32>,
        input: &dyn Fn(&str) -> crate::nodes::Extent,
    ) -> Result<crate::nodes::Extent> {
        Ok(input("image"))
    }
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Fade)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        matches!(self.opacity, Scalar::Constant { value } if value == 1.0)
            && self.timing.always_visible()
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values[0] = (f64::from(sample("opacity")?) * self.timing.envelope(cx.time)?) as f32;
        Ok(())
    }
    /// Fully faded out: the picture is clear, so the fade itself is not run either.
    fn empty(
        &self,
        sample: &dyn Fn(&str) -> Result<f32>,
        time: veylvik_document::Time,
    ) -> Result<bool> {
        Ok(f64::from(sample("opacity")?) * self.timing.envelope(time)? == 0.0)
    }
    /// Fully faded out: nothing upstream is rendered or decoded.
    fn idle_inputs(
        &self,
        sample: &dyn Fn(&str) -> Result<f32>,
        time: veylvik_document::Time,
    ) -> Result<&'static [&'static str]> {
        let visible = f64::from(sample("opacity")?) * self.timing.envelope(time)?;
        Ok(if visible == 0.0 { &["image"] } else { &[] })
    }
}
