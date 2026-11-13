//! Colour adjustments applied per pixel to one image.
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exposure {
    pub stops: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colorize {
    pub low: [f32; 4],
    pub high: [f32; 4],
}

node!(Exposure, EXPOSURE, [stops]);
node!(Colorize, COLORIZE, [], {
    fn validate(&self) -> Result<()> {
        if [self.low, self.high]
            .iter()
            .any(|color| color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3]))
        {
            return fail("colorize needs finite RGB and alpha in [0,1] for both endpoints");
        }
        Ok(())
    }
});
