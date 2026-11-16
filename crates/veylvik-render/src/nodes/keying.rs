//! GPU passes and uniforms for keyers.
use super::{Cx, From, Passes, RenderNode, Step};
use crate::{Kernel, Params, Result};

impl RenderNode for veylvik_document::nodes::ChromaKey {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::ChromaKey)
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
        let [r, g, b, _] = self.key_color;
        p.values = [r, g, b, sample("tolerance")?];
        p.extra = [sample("softness")?, sample("despill")?, 0.0, 0.0];
        Ok(())
    }
}
impl RenderNode for veylvik_document::nodes::LumaKey {
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
        step(FuseFinish, S(15), S(14), N),
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
        let separation = 0.25 - 0.21 * sample("detail")?.clamp(0.0, 1.0);
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
