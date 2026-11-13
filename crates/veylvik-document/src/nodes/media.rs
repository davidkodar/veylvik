//! Still images and footage read from project assets.
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub asset: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footage {
    pub asset: String,
    pub timing: media::Timing,
    #[serde(default)]
    pub placement: media::Placement,
}

node!(Image, IMAGE, [], {
    fn validate(&self) -> Result<()> {
        let asset = &self.asset;
        if asset.trim().is_empty()
            || asset.chars().count() > crate::registry::MAX_ASSET_NAME
            || asset.contains('\0')
        {
            return fail("image needs a nonempty asset key without NUL, at most 4096 characters");
        }
        Ok(())
    }
    fn asset(&self) -> Option<&str> {
        Some(&self.asset)
    }
    fn asset_mut(&mut self) -> Option<&mut String> {
        Some(&mut self.asset)
    }
});
node!(Footage, FOOTAGE, [], {
    fn validate(&self) -> Result<()> {
        let asset = &self.asset;
        if asset.trim().is_empty()
            || asset.chars().count() > crate::registry::MAX_ASSET_NAME
            || asset.contains('\0')
        {
            return fail("Footage needs a valid asset key");
        }
        self.timing.validate()
    }
    fn asset(&self) -> Option<&str> {
        Some(&self.asset)
    }
    fn asset_mut(&mut self) -> Option<&mut String> {
        Some(&mut self.asset)
    }
});
