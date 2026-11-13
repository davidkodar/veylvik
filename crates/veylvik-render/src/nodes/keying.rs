//! GPU passes and uniforms for keyers.
use super::{Cx, Passes, RenderNode};
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
