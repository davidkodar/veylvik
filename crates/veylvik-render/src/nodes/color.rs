//! GPU passes and uniforms for colour adjustment nodes.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Exposure {
    // Colour multiplied in place; clear stays clear.
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
        Passes::One(Kernel::Exposure)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        let zero = |value: &Scalar| matches!(value, Scalar::Constant { value } if *value == 0.0);
        let veylvik_document::nodes::Exposure { stops } = self;
        zero(stops)
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
            let stops = sample("stops")?;
            p.values[0] = stops.exp2();
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Colorize {
    // Coloured in place by its own brightness; clear stays clear.
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
        Passes::One(Kernel::Colorize)
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
        let veylvik_document::nodes::Colorize { low, high } = self;
        {
            p.values = *low;
            p.extra = *high;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::ColorCorrect {
    // Graded in place; a clear pixel is passed on as it is.
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
        Passes::One(Kernel::ColorCorrect)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        let at = |value: &Scalar, default: f32| matches!(value, Scalar::Constant { value } if *value == default);
        at(&self.temperature, 0.0)
            && at(&self.tint, 0.0)
            && at(&self.lift, 0.0)
            && at(&self.gain, 1.0)
            && at(&self.contrast, 1.0)
            && at(&self.gamma, 1.0)
            && at(&self.saturation, 1.0)
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values = [
            sample("temperature")?,
            sample("tint")?,
            sample("lift")?,
            sample("gain")?,
        ];
        p.extra = [
            sample("contrast")?,
            sample("gamma")?,
            sample("saturation")?,
            0.0,
        ];
        Ok(())
    }
}

impl RenderNode for veylvik_document::nodes::Lut {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Lut)
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        use veylvik_document::media::{Curve, Gamut};
        let curve = |c: Curve| Curve::ALL.iter().position(|x| *x == c).unwrap_or(0) as u32;
        let gamut = |g: Gamut| Gamut::ALL.iter().position(|x| *x == g).unwrap_or(0) as u32;
        let packed = curve(self.input.curve)
            | gamut(self.input.gamut) << 4
            | curve(self.output.curve) << 8
            | gamut(self.output.gamut) << 12;
        // values.x/y and extra are filled from the LUT asset by the renderer.
        p.values[2] = sample("mix")?.clamp(0.0, 1.0);
        p.values[3] = packed as f32;
        Ok(())
    }
}

impl RenderNode for veylvik_document::nodes::Ocio {}
