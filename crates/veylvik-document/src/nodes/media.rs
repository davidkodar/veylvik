//! Still images and footage read from project assets.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
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
    optional: &[],
    output: PortKind::Image,
    parameters: &[other("asset", ParameterKind::Asset, "asset key")],
};
pub static FOOTAGE: NodeType = NodeType {
    type_id: "footage",
    version: 1,
    label: "Footage",
    description: "Time-aware video or image sequence, with exact source in/out and timeline placement. Outside the clip is transparent; pixels retain native dimensions. colour overrides a video's colour tags: curve (auto, rec709, srgb, linear, pq, hlg, apple_log, slog3, logc3, logc4, vlog) and gamut (auto, rec709, rec2020, p3_d65, s_gamut3, s_gamut3_cine, arri_wide_gamut3, arri_wide_gamut4, v_gamut); camera log footage is usually untagged and needs its curve and gamut set.",
    inputs: &[],
    optional: &[],
    output: PortKind::Image,
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

/// Exposure sampling of the upstream branch, in composition frame intervals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionBlur {
    pub angle: Scalar,
    pub phase: Scalar,
    pub preview_samples: Scalar,
    pub final_samples: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeRemap {
    /// Source seconds = timeline seconds * rate + source_time.
    /// For animated remapping set rate to zero and keyframe source_time.
    pub rate: Scalar,
    pub source_time: Scalar,
}
pub static MOTION_BLUR: NodeType = NodeType {
    type_id: "motion_blur",
    version: 1,
    label: "Motion blur",
    description: "GPU shutter sampling of this upstream branch only. Place after animated transforms or warps. Footage may already contain exposure blur. Preview and final sample counts are separate; nested temporal work is bounded.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("angle", "degrees", 0.0, 360.0, 180.0),
        scalar("phase", "frames", -0.5, 0.5, 0.0),
        scalar("preview_samples", "samples", 1.0, 32.0, 4.0),
        scalar("final_samples", "samples", 1.0, 32.0, 16.0),
    ],
};
pub static TIME_REMAP: NodeType = NodeType {
    type_id: "time_remap",
    version: 1,
    label: "Time remap",
    description: "Retimes this upstream branch only. Source seconds = timeline seconds times rate plus source_time. Rate 0 freezes at source_time; negative rate reverses. For speed ramps set rate to zero and animate source_time (its slope is speed). Footage retains its own trim and transparent boundaries. Does not retime audio or synthesize intermediate footage frames.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("rate", "factor", -100.0, 100.0, 1.0),
        scalar("source_time", "seconds", -86400.0, 86400.0, 0.0),
    ],
};
node!(
    MotionBlur,
    MOTION_BLUR,
    [angle, phase, preview_samples, final_samples]
);
node!(TimeRemap, TIME_REMAP, [rate, source_time]);

impl TimeRemap {
    /// Rates use millionths, preserving exact frame boundaries for common playback
    /// speeds. Offset seconds use nanoseconds; overflow is reported, never saturated.
    pub fn source_at(&self, time: Time) -> Result<Time> {
        let rate = self.rate.at(time)?;
        let offset = self.source_time.at(time)?;
        if !rate.is_finite() || !offset.is_finite() || rate.abs() > 100.0 || offset.abs() > 86400.0
        {
            return fail("Invalid time remap parameters");
        }
        let numerator = i128::from(time.ticks) * (f64::from(rate) * 1e6).round() as i128;
        let denominator = i128::from(time.rate) * 1_000_000;
        if denominator == 0 {
            return fail("Invalid timeline time");
        }
        let (mut a, mut b) = (numerator.abs(), denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let scaled = Time::new(
            i64::try_from(numerator / a)
                .map_err(|_| crate::Error("Remapped time overflow".into()))?,
            u32::try_from(denominator / a).map_err(|_| {
                crate::Error("Remapped time precision exceeds timestamp capacity".into())
            })?,
        )?;
        scaled.checked_add(Time::new(
            (f64::from(offset) * 1e9).round() as i64,
            1_000_000_000,
        )?)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowQuality {
    Fast,
    #[default]
    Balanced,
    Fine,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpticalFlow {
    pub rate: Scalar,
    pub source_time: Scalar,
    pub source_rate: Time,
    pub confidence: Scalar,
    pub quality: FlowQuality,
    pub diagnostic: bool,
}
pub static OPTICAL_FLOW: NodeType = NodeType {
    type_id: "optical_flow",
    version: 1,
    label: "Optical flow retime",
    description: "Retimes an upstream image branch using GPU bidirectional pyramidal optical flow. Source rate must match the original clip cadence. Forward/backward consistency rejects uncertain or occluded regions, which use the nearest source frame; major cuts also fall back. Confidence view shows reliable regions in green. Reduced analysis resolution in preview; output retains full composition dimensions. No audio retiming or vector motion blur.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("rate", "factor", -100.0, 100.0, 0.5),
        scalar("source_time", "seconds", -86400.0, 86400.0, 0.0),
        other("source_rate", ParameterKind::FrameRate, "fps"),
        scalar("confidence", "factor", 0.01, 1.0, 0.15),
        other("quality", ParameterKind::FlowQuality, ""),
        other("diagnostic", ParameterKind::Boolean, ""),
    ],
};
node!(
    OpticalFlow,
    OPTICAL_FLOW,
    [rate, source_time, confidence],
    {
        fn validate(&self) -> Result<()> {
            if !(1.0..=1000.0).contains(&self.source_rate.seconds()?) {
                return fail("Optical flow source frame rate must be 1..1000 fps");
            }
            Ok(())
        }
    }
);
impl OpticalFlow {
    pub fn source_pair(&self, time: Time) -> Result<(Time, Time, f32)> {
        let mapped = TimeRemap {
            rate: self.rate.clone(),
            source_time: self.source_time.clone(),
        }
        .source_at(time)?;
        let num = i128::from(mapped.ticks) * i128::from(self.source_rate.ticks);
        let den = i128::from(mapped.rate) * i128::from(self.source_rate.rate);
        if den <= 0 || self.source_rate.ticks <= 0 {
            return fail("Invalid source frame rate");
        }
        let frame = i64::try_from(num.div_euclid(den))
            .map_err(|_| crate::Error("Source frame index overflow".into()))?;
        let first = Time::at_frame(frame, self.source_rate)?;
        let second = Time::at_frame(
            frame
                .checked_add(1)
                .ok_or_else(|| crate::Error("Source frame index overflow".into()))?,
            self.source_rate,
        )?;
        Ok((
            first,
            second,
            (num.rem_euclid(den) as f64 / den as f64) as f32,
        ))
    }
}
