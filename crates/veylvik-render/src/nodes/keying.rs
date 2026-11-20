//! GPU passes and uniforms for keyers.
use super::{Cx, From, Passes, RenderNode, Step};
use crate::{Kernel, Params, Result};

impl RenderNode for veylvik_document::nodes::ChromaKey {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::ChromaKey)
    }
    /// The plain key is one pass, exactly as it always was. Shaping the matte, masks
    /// and the matte view add passes: the matte alone, shrink, soften, then the picture.
    fn passes_for(&self, node: &veylvik_document::Node) -> Passes {
        use super::{From, Step};
        use veylvik_document::nodes::KeyView;
        let rests = |scalar: &veylvik_document::Scalar, at: f32| matches!(scalar, veylvik_document::Scalar::Constant { value } if *value == at);
        let (keep, garbage) = (
            node.inputs.contains_key("keep"),
            node.inputs.contains_key("garbage"),
        );
        let (shrink, soften) = (!rests(&self.shrink, 0.0), !rests(&self.soften, 0.0));
        if rests(&self.darks, 0.0)
            && rests(&self.clip_black, 0.0)
            && rests(&self.clip_white, 1.0)
            && !shrink
            && !soften
            && !keep
            && !garbage
            && self.show == KeyView::Result
        {
            return self.passes();
        }
        let step = |kernel, from| Step {
            kernel,
            inputs: [from, From::None, From::None, From::None],
        };
        let mut steps = vec![step(Kernel::ChromaMatte, From::Port(0))];
        if shrink {
            steps.push(step(
                Kernel::KeyShrinkHorizontal,
                From::Step(steps.len() - 1),
            ));
            steps.push(step(Kernel::KeyShrinkVertical, From::Step(steps.len() - 1)));
        }
        if soften {
            steps.push(step(
                Kernel::KeySoftenHorizontal,
                From::Step(steps.len() - 1),
            ));
            steps.push(step(Kernel::KeySoftenVertical, From::Step(steps.len() - 1)));
        }
        steps.push(Step {
            kernel: Kernel::ChromaApply,
            inputs: [
                From::Port(0),
                From::Step(steps.len() - 1),
                if keep { From::Port(1) } else { From::None },
                if garbage { From::Port(2) } else { From::None },
            ],
        });
        Passes::Chain(steps.into())
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
        let [r, g, b, _] = self.key_color;
        p.values = [r, g, b, sample("tolerance")?];
        // The single-pass key reads only the first two of `extra`.
        p.extra = [
            sample("softness")?,
            sample("despill")?,
            sample("clip_black")?,
            sample("clip_white")?,
        ];
        let shrink = sample("shrink")?.round();
        p.size_op[2] = sample("darks")?.to_bits();
        p.size_op[3] = (shrink.abs() as u32).min(32)
            | u32::from(shrink < 0.0) << 6
            | (sample("soften")?.round() as u32).min(32) << 7
            | u32::from(self.show == veylvik_document::nodes::KeyView::Matte) << 13
            | u32::from(cx.node.inputs.contains_key("keep")) << 14
            | u32::from(cx.node.inputs.contains_key("garbage")) << 15;
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::LumaKey {
    // Kept or cut by its own brightness; clear stays clear.
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
        Passes::One(Kernel::LumaKey)
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
        p.values[0] = sample("low")?;
        p.values[1] = sample("high")?;
        Ok(())
    }
}

/// Colour-projection edge snapping, then blur-fusion foreground estimation (Forte & Pitié).
/// Step 8 carries the image's straight colour with the refined alpha through the fusion.
const REFINE: &[Step] = {
    use From::{None as N, Port as P, Step as S};
    use Kernel::*;
    &[
        // 0: [encoded colour, coarse matte]
        step(MattePack, P(0), P(1), N),
        // 1–3: local colour of the confident subject; 4–6: of the confident background
        step(KnownSubject, S(0), N, N),
        step(MatteBoxHorizontal, S(1), N, N),
        step(MatteBoxVertical, S(2), N, N),
        step(KnownBackground, S(0), N, N),
        step(MatteBoxHorizontal, S(4), N, N),
        step(MatteBoxVertical, S(5), N, N),
        // 7: refined alpha
        step(MatteProject, S(3), S(6), S(0)),
        // 8: [straight image colour, refined alpha]
        step(MatteJoin, S(7), P(0), N),
        step(FuseForeground, S(8), N, N),
        step(FuseBoxHorizontal, S(9), N, N),
        step(FuseBoxVertical, S(10), N, N),
        step(FuseBackground, S(8), N, N),
        step(FuseBoxHorizontal, S(12), N, N),
        step(FuseBoxVertical, S(13), N, N),
        step(FuseEstimate, S(11), S(8), N),
        step(FuseFinish, S(15), S(14), S(11)),
    ]
};
const fn step(kernel: Kernel, a: From, b: From, c: From) -> Step {
    Step {
        kernel,
        inputs: [a, b, c, From::None],
    }
}
impl RenderNode for veylvik_document::nodes::RefineMatte {
    fn passes(&self) -> Passes {
        Passes::chain(REFINE)
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
        let radius = sample("radius")?.round().clamp(1.0, 64.0);
        // Detail 0..1 lowers the colour difference (in sqrt-encoded RGB) that subject and
        // background need before their edge is re-estimated, from 0.25 to 0.04.
        // At Detail 0 the matte's own alpha stands (for a matte a matting model made).
        let detail = sample("detail")?.clamp(0.0, 1.0);
        let separation = if detail > 0.0 {
            0.25 - 0.21 * detail
        } else {
            1e9
        };
        // Colours are estimated from a wider neighbourhood than the edge window.
        let fusion = (radius * 4.0).clamp(8.0, 96.0);
        p.values = [
            radius,
            fusion,
            separation,
            sample("decontaminate")?.clamp(0.0, 1.0),
        ];
        Ok(())
    }
}
