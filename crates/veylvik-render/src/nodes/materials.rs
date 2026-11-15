//! GPU passes and uniforms for procedural material nodes.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Material {
    fn passes(&self) -> Passes {
        // A smooth height field first; the material shades its gradient as the bevel.
        Passes::Chain(&[
            super::Step {
                kernel: Kernel::BevelHeightHorizontal,
                inputs: [super::From::Port(0), super::From::None, super::From::None],
            },
            super::Step {
                kernel: Kernel::BevelHeightVertical,
                inputs: [super::From::Step(0), super::From::None, super::From::None],
            },
            super::Step {
                kernel: Kernel::Material,
                inputs: [
                    super::From::Port(0),
                    super::From::Step(1),
                    super::From::None,
                ],
            },
        ])
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
        let veylvik_document::nodes::Material { kind, .. } = self;
        {
            p.size_op[3] = *kind as u32;
            p.values = [
                sample("scale")?,
                sample("bevel")?,
                sample("roughness")?,
                sample("lighting")?.rem_euclid(1.0),
            ];
            p.extra = [
                sample("grain")?,
                sample("emission")?,
                if self.smooth_edges { 0.0 } else { -1.0 },
                f32::from(self.is_brushed()),
            ];
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Patina {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Patina)
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
        let veylvik_document::nodes::Patina { kind, seed, .. } = self;
        {
            p.size_op[2] = *seed;
            p.size_op[3] = *kind as u32;
            p.values = [
                sample("amount")?,
                sample("dirt")?,
                sample("scale")?,
                sample("wear")?,
            ];
            p.extra[0] = sample("scratches")?;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::MetallicShine {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::MetallicShine)
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
                sample("width")?,
                sample("angle")?.to_radians(),
            ];
            p.extra[0] = sample("intensity")?;
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::SunGlints {
    fn passes(&self) -> Passes {
        Passes::Chain(&[
            super::Step {
                kernel: Kernel::GlintSeeds,
                inputs: [super::From::Port(0), super::From::None, super::From::None],
            },
            super::Step {
                kernel: Kernel::SunGlints,
                inputs: [
                    super::From::Port(0),
                    super::From::Step(0),
                    super::From::None,
                ],
            },
        ])
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
        let veylvik_document::nodes::SunGlints { seed, .. } = self;
        {
            p.size_op[3] = *seed;
            p.values = [
                sample("size")?,
                sample("intensity")?,
                sample("threshold")?,
                sample("evolution")?.rem_euclid(4.0),
            ];
            p.extra = [sample("density")?, sample("angle")?.to_radians(), 0.0, 0.0];
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Glass {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Glass)
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
                sample("bevel")?,
                sample("refraction")?,
                sample("roughness")?,
                sample("ior")?,
            ];
            p.extra = [
                sample("transmission")?,
                sample("tint")?,
                sample("dispersion")?,
                0.0,
            ];
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::SurfaceReflection {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::SurfaceReflection)
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
        let veylvik_document::nodes::SurfaceReflection { kind, .. } = self;
        {
            p.size_op[3] = *kind as u32;
            p.values = [
                sample("bevel")?,
                sample("curvature")?,
                sample("roughness")?,
                sample("strength")?,
            ];
            p.extra = [
                sample("rotation")?.to_radians(),
                sample("scale")?,
                sample("offset_x")?,
                sample("offset_y")?,
            ];
        }
        Ok(())
    }
}

impl RenderNode for veylvik_document::nodes::Condensation {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Condensation)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.size_op[3] = self.seed;
        p.values = [
            sample("amount")?,
            sample("size")?,
            sample("density")?,
            sample("haze")?,
        ];
        p.extra = [
            sample("evolution")?,
            sample("trails")?,
            sample("dispersion")?,
            0.0,
        ];
        Ok(())
    }
}
