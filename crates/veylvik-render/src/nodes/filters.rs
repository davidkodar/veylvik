//! GPU passes and uniforms for spatial filters and transforms.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Blur {
    fn passes(&self) -> Passes {
        Passes::Separable(Kernel::BlurHorizontal, Kernel::BlurVertical)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        let zero = |value: &Scalar| matches!(value, Scalar::Constant { value } if *value == 0.0);
        let veylvik_document::nodes::Blur { radius, .. } = self;
        zero(radius)
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
            p.values[0] = sample("radius")?;
            p.values[1] = match self.filter {
                veylvik_document::nodes::BlurFilter::Box => 0.0,
                veylvik_document::nodes::BlurFilter::Gaussian => 1.0,
            };
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::LensBlur {
    fn passes(&self) -> Passes {
        use super::{From, Step};
        // Boost and prefilter to the sample spacing, then gather over the disc.
        static STEPS: [Step; 3] = [
            Step {
                kernel: Kernel::LensPrefilterHorizontal,
                inputs: [From::Port(0), From::None, From::None, From::None],
            },
            Step {
                kernel: Kernel::LensPrefilterVertical,
                inputs: [From::Step(0), From::None, From::None, From::None],
            },
            Step {
                kernel: Kernel::LensBlur,
                inputs: [From::Step(1), From::None, From::None, From::None],
            },
        ];
        Passes::chain(&STEPS)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        matches!(self.radius, Scalar::Constant { value } if value < 0.5)
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values[0] = sample("radius")?;
        p.values[1] = sample("highlights")?;
        p.values[2] = sample("threshold")?;
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Morphology {
    fn passes(&self) -> Passes {
        Passes::Separable(Kernel::MorphologyHorizontal, Kernel::MorphologyVertical)
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
            p.values[0] = sample("radius")?.round();
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Displace {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Displace)
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
            p.values[0] = sample("x")?;
            p.values[1] = sample("y")?;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Transform {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Transform)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        let zero = |value: &Scalar| matches!(value, Scalar::Constant { value } if *value == 0.0);
        let veylvik_document::nodes::Transform { x, y } = self;
        zero(x) && zero(y)
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
            p.values[0] = sample("x")?;
            p.values[1] = sample("y")?;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Affine {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Affine)
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
            let (sx, sy) = (sample("scale_x")? as f64, sample("scale_y")? as f64);
            if sx.abs() >= 1e-6_f32 as f64 && sy.abs() >= 1e-6_f32 as f64 {
                let radians = (sample("rotation")? as f64).rem_euclid(360.0).to_radians();
                let (sin, cos) = radians.sin_cos();
                let (px, py) = (sample("pivot_x")? as f64, sample("pivot_y")? as f64);
                let (tx, ty) = (px + sample("x")? as f64, py + sample("y")? as f64);
                let m = [cos / sx, sin / sx, -sin / sy, cos / sy];
                p.values = m.map(|v| v as f32);
                p.extra = [
                    (px - m[0] * tx - m[1] * ty) as f32,
                    (py - m[2] * tx - m[3] * ty) as f32,
                    1.0,
                    0.0,
                ];
            }
        }
        Ok(())
    }
}

impl RenderNode for veylvik_document::nodes::ContactShadow {
    fn passes(&self) -> Passes {
        use super::{From, Step};
        // A gaussian of the layer's alpha over the reach carries the soft outer
        // falloff; the occlusion pass adds the tight contact from the sharp alpha.
        Passes::chain(&[
            Step {
                kernel: Kernel::BlurHorizontal,
                inputs: [From::Port(0), From::None, From::None, From::None],
            },
            Step {
                kernel: Kernel::BlurVertical,
                inputs: [From::Step(0), From::None, From::None, From::None],
            },
            Step {
                kernel: Kernel::ContactShadow,
                inputs: [From::Port(0), From::Step(1), From::None, From::None],
            },
        ])
    }
    fn shareable(&self) -> bool {
        true
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let reach = sample("reach")?;
        // values.x/y are the blur's radius (its visible reach) and gaussian flag.
        p.values = [reach.clamp(0.0, 256.0), 1.0, reach, sample("strength")?];
        let follows = crate::shadow_lighting::material(cx.node, cx.nodes).is_some();
        p.extra = [
            sample("lean")?,
            sample("angle")?.to_radians(),
            f32::from(follows),
            0.,
        ];
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Shadow {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Shadow)
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values = [
            sample("x")?,
            sample("y")?,
            sample("softness")?,
            sample("strength")?,
        ];
        p.extra = [
            sample("distance")?,
            f32::from(self.follow_lighting),
            sample("light")?.round(),
            0.,
        ];
        Ok(())
    }
}
