//! GPU passes and uniforms for colour adjustment nodes.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::Exposure {
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
