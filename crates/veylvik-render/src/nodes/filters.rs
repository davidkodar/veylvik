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
    fn passes_for(&self, node: &veylvik_document::Node) -> Passes {
        use super::{From, Step};
        if !node.inputs.contains_key("depth") {
            return self.passes();
        }
        // Each pixel's disc comes from the depth pass on the second input.
        static STEPS: [Step; 3] = [
            Step {
                kernel: Kernel::LensDepthPrefilterHorizontal,
                inputs: [From::Port(0), From::Port(1), From::None, From::None],
            },
            Step {
                kernel: Kernel::LensDepthPrefilterVertical,
                inputs: [From::Step(0), From::Port(1), From::None, From::None],
            },
            Step {
                kernel: Kernel::LensDepthBlur,
                inputs: [From::Step(1), From::Port(1), From::None, From::None],
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
        p.values[3] = sample("focus")?;
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
impl RenderNode for veylvik_document::nodes::MatchGrain {
    /// The plate is only measured, in Studio; rendering never reads it.
    fn idle_inputs(
        &self,
        _sample: &dyn Fn(&str) -> Result<f32>,
        _time: veylvik_document::Time,
    ) -> Result<&'static [&'static str]> {
        Ok(&["plate"])
    }
    fn identity(&self) -> bool {
        matches!(self.amount, Scalar::Constant { value } if value == 0.0)
    }
    fn passes(&self) -> Passes {
        use super::{From, Step};
        // White noise, blurred to the grain size, added by brightness.
        Passes::chain(&[
            Step {
                kernel: Kernel::GrainNoise,
                inputs: [From::Port(0), From::None, From::None, From::None],
            },
            Step {
                kernel: Kernel::BlurHorizontal,
                inputs: [From::Step(0), From::None, From::None, From::None],
            },
            Step {
                kernel: Kernel::BlurVertical,
                inputs: [From::Step(1), From::None, From::None, From::None],
            },
            Step {
                kernel: Kernel::GrainApply,
                inputs: [From::Port(0), From::Step(2), From::None, From::None],
            },
        ])
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        // Fresh grain every frame: the seed counts 240ths of a second.
        let seconds = cx.time.seconds().map_err(|e| error(e.to_string()))?;
        p.size_op[2] = (seconds * 240.0).round().rem_euclid(f64::from(u32::MAX)) as u32;
        let amount = sample("amount")?;
        p.values = [
            sample("size")?.round().clamp(0.0, 9.0),
            1.0,
            sample("colour")?,
            sample("red")?,
        ];
        p.extra = [
            sample("shadows")? * amount,
            sample("midtones")? * amount,
            sample("highlights")? * amount,
            sample("blue")?,
        ];
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::LensDistortion {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::LensDistortion)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        let zero = |s: &Scalar| matches!(s, Scalar::Constant { value } if *value == 0.0);
        zero(&self.k1) && zero(&self.k2)
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let (k1, k2) = (sample("k1")?, sample("k2")?);
        let zoom = if self.fill {
            Self::fill_zoom(k1, k2, self.redistort, cx.width, cx.height)
        } else {
            1.0
        };
        p.values = [k1, k2, f32::from(self.redistort), zoom];
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Bloom {
    fn passes(&self) -> Passes {
        use super::{From, Step};
        const fn step(kernel: Kernel, a: From, b: From, c: From) -> Step {
            Step {
                kernel,
                inputs: [a, b, c, From::None],
            }
        }
        // The combine also takes the sideways-only blur, for lens streaks.
        const COMBINE: Step = Step {
            kernel: Kernel::BloomCombine,
            inputs: [From::Port(0), From::Step(2), From::Step(4), From::Step(5)],
        };
        // The bright part, blurred wide and tight, added back to the image.
        static STEPS: [Step; 7] = [
            step(Kernel::BloomBright, From::Port(0), From::None, From::None),
            step(
                Kernel::BloomWideHorizontal,
                From::Step(0),
                From::None,
                From::None,
            ),
            step(
                Kernel::BloomWideVertical,
                From::Step(1),
                From::None,
                From::None,
            ),
            step(
                Kernel::BloomTightHorizontal,
                From::Step(0),
                From::None,
                From::None,
            ),
            step(
                Kernel::BloomTightVertical,
                From::Step(3),
                From::None,
                From::None,
            ),
            step(
                Kernel::BloomStreakVertical,
                From::Step(1),
                From::None,
                From::None,
            ),
            COMBINE,
        ];
        Passes::chain(&STEPS)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        matches!(self.intensity, Scalar::Constant { value } if value == 0.0)
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values = [
            sample("radius")?,
            sample("intensity")?,
            sample("threshold")?,
            sample("knee")?,
        ];
        p.extra = [self.tint[0], self.tint[1], self.tint[2], sample("streak")?];
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::ZoomBlur {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::ZoomBlur)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        matches!(self.amount, Scalar::Constant { value } if value == 0.0)
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values = [sample("amount")?, sample("x")?, sample("y")?, 0.0];
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::LightWrap {
    fn passes(&self) -> Passes {
        use super::{From, Step};
        const fn step(kernel: Kernel, a: From, b: From, c: From) -> Step {
            Step {
                kernel,
                inputs: [a, b, c, From::None],
            }
        }
        // Bloom's wide blur is a plain gaussian of values.x pixels: here it blurs the
        // background (port 1), then the foreground (port 0), for the combine.
        static STEPS: [Step; 5] = [
            step(
                Kernel::BloomWideHorizontal,
                From::Port(1),
                From::None,
                From::None,
            ),
            step(
                Kernel::BloomWideVertical,
                From::Step(0),
                From::None,
                From::None,
            ),
            step(
                Kernel::BloomWideHorizontal,
                From::Port(0),
                From::None,
                From::None,
            ),
            step(
                Kernel::BloomWideVertical,
                From::Step(2),
                From::None,
                From::None,
            ),
            step(
                Kernel::LightWrapCombine,
                From::Port(0),
                From::Step(1),
                From::Step(3),
            ),
        ];
        Passes::chain(&STEPS)
    }
    fn shareable(&self) -> bool {
        true
    }
    fn identity(&self) -> bool {
        matches!(self.amount, Scalar::Constant { value } if value == 0.0)
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values = [sample("radius")?, sample("amount")?, 0.0, 0.0];
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::Shader {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::Custom)
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        // The frame's time for the shader, and which inputs are connected.
        let seconds = cx.time.seconds().map_err(|e| error(e.to_string()))?;
        p.size_op[2] = (seconds as f32).to_bits();
        p.size_op[3] = u32::from(cx.node.inputs.contains_key("image"))
            | u32::from(cx.node.inputs.contains_key("second")) << 1;
        p.values = [sample("p1")?, sample("p2")?, sample("p3")?, sample("p4")?];
        p.extra = [sample("p5")?, sample("p6")?, sample("p7")?, sample("p8")?];
        for (value, name) in p
            .more
            .iter_mut()
            .zip(["p9", "p10", "p11", "p12", "p13", "p14", "p15", "p16"])
        {
            *value = sample(name)?;
        }
        // Where the followed object is in the frame, through the scene's first camera.
        if let Some(target) = self.follow {
            p.anchor = [0.0, 0.0, -1.0, 0.0];
            if let Some(root) = cx.node.inputs.get("scene") {
                let by_id: std::collections::BTreeMap<_, _> =
                    cx.nodes.iter().map(|n| (n.id, n)).collect();
                let lookup = |id| by_id.get(&id).copied();
                let scene = veylvik_scene::evaluate_in(
                    &veylvik_scene::Graph {
                        width: cx.width,
                        height: cx.height,
                        node: &lookup,
                    },
                    *root,
                    cx.time,
                )
                .map_err(error)?;
                let camera = scene.camera(1, cx.width, cx.height);
                if let Some((at, _)) = scene
                    .centre(target)
                    .and_then(|centre| camera.project(centre, cx.width, cx.height))
                {
                    p.anchor = [at[0], at[1], 1.0, 0.0];
                }
            }
        }
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::GridWarp {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::GridWarp)
    }
    fn identity(&self) -> bool {
        self.points.iter().all(|p| *p == [0.0; 2])
            || matches!(self.amount, Scalar::Constant { value } if value == 0.0)
    }
    /// The mesh's offsets as a columns x rows image; one image per distinct mesh.
    fn has_data(&self) -> bool {
        true
    }
    fn data(&self, _cx: &Cx<'_>) -> Result<Option<std::sync::Arc<crate::Image>>> {
        Ok(grid_mesh(self))
    }
    fn params(
        &self,
        _cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        p.values = [
            self.columns as f32,
            self.rows as f32,
            sample("amount")?,
            0.0,
        ];
        Ok(())
    }
}
/// The mesh's offsets as a columns x rows image; one image per distinct mesh.
fn grid_mesh(warp: &veylvik_document::nodes::GridWarp) -> Option<std::sync::Arc<crate::Image>> {
    type Kept = Vec<((u32, Vec<[f32; 2]>), std::sync::Arc<crate::Image>)>;
    static MESHES: std::sync::Mutex<Kept> = std::sync::Mutex::new(Vec::new());
    let key = (warp.columns, warp.points.clone());
    let mut kept = MESHES.lock().ok()?;
    if let Some((_, image)) = kept.iter().find(|(k, _)| *k == key) {
        return Some(image.clone());
    }
    let image = std::sync::Arc::new(
        crate::Image::new(
            warp.columns,
            warp.rows,
            warp.points.iter().map(|p| [p[0], p[1], 0.0, 1.0]).collect(),
        )
        .ok()?,
    );
    if kept.len() >= 64 {
        kept.remove(0);
    }
    kept.push((key, image.clone()));
    Some(image)
}
impl RenderNode for veylvik_document::nodes::ShapeWarp {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::GridWarp)
    }
    fn identity(&self) -> bool {
        (self.from.keys.is_empty()
            && self.to.keys.is_empty()
            && self.from.commands == self.to.commands)
            || matches!(self.amount, Scalar::Constant { value } if value == 0.0)
    }
    fn has_data(&self) -> bool {
        true
    }
    /// The mesh that carries the start shape onto the end shape at `time`; the same
    /// shapes give the same image, so a still warp stays cached.
    fn data(&self, cx: &Cx<'_>) -> Result<Option<std::sync::Arc<crate::Image>>> {
        let (time, width, height) = (cx.time, cx.width, cx.height);
        type Key = (
            Vec<veylvik_document::vector::PathCommand>,
            Vec<veylvik_document::vector::PathCommand>,
            [u32; 3],
        );
        static FIELDS: std::sync::Mutex<Vec<(Key, std::sync::Arc<crate::Image>)>> =
            std::sync::Mutex::new(Vec::new());
        let (from, to) = (self.from.at(time)?, self.to.at(time)?);
        let reach = self.reach.at(time)?;
        let key = (
            from.commands.clone(),
            to.commands.clone(),
            [width, height, reach.to_bits()],
        );
        let mut kept = FIELDS
            .lock()
            .map_err(|_| error("warp field lock poisoned"))?;
        if let Some((_, image)) = kept.iter().find(|(k, _)| *k == key) {
            return Ok(Some(image.clone()));
        }
        let field = veylvik_warp::field(&from, &to, width, height, reach).map_err(error)?;
        let image = std::sync::Arc::new(crate::Image::new(
            field.columns,
            field.rows,
            field
                .offsets
                .iter()
                .map(|o| [o[0], o[1], 0.0, 1.0])
                .collect(),
        )?);
        if kept.len() >= 16 {
            kept.remove(0);
        }
        kept.push((key, image.clone()));
        Ok(Some(image))
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let (columns, rows) = veylvik_warp::sides(cx.width, cx.height);
        p.values = [columns as f32, rows as f32, sample("amount")?, 0.0];
        Ok(())
    }
}
