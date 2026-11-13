//! Still images and footage read from project assets.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
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
    /// Overrides the video's colour tags; camera log footage is usually untagged. Projects
    /// from before it existed load as Auto.
    #[serde(default)]
    pub colour: media::InputColour,
}

pub static IMAGE: NodeType = NodeType {
    type_id: "image",
    version: 1,
    label: "Image",
    description: "Host-supplied linear premultiplied image at native size, top-left anchored, cropped or padded to the composition.",
    inputs: &[],
    parameters: &[other("asset", ParameterKind::Asset, "asset key")],
};
pub static FOOTAGE: NodeType = NodeType {
    type_id: "footage",
    version: 1,
    label: "Footage",
    description: "Time-aware video or image sequence, with exact source in/out and timeline placement. Outside the clip is transparent; pixels retain native dimensions. colour overrides a video's colour tags: curve (auto, rec709, srgb, linear, pq, hlg, apple_log, slog3, logc3, logc4, vlog) and gamut (auto, rec709, rec2020, p3_d65, s_gamut3, s_gamut3_cine, arri_wide_gamut3, arri_wide_gamut4, v_gamut); camera log footage is usually untagged and needs its curve and gamut set.",
    inputs: &[],
    parameters: &[
        other("asset", ParameterKind::Asset, "asset key"),
        other("timing", ParameterKind::MediaTiming, "seconds"),
        other("placement", ParameterKind::MediaPlacement, ""),
        other("colour", ParameterKind::MediaColour, ""),
    ],
};

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
