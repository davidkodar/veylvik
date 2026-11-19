//! Sound on the timeline: an audio file placed like a clip, heard in preview and mixed into video exports. It makes no picture and connects to nothing.
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
use crate::*;
use serde::{Deserialize, Serialize};

/// An audio file (or a video's sound) on the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sound {
    /// The file, absolute or relative to the project, or `builtin:NAME` for a sound
    /// Veylvik ships.
    pub file: String,
    #[serde(default)]
    pub timing: media::Timing,
    /// Level in decibels, read where the sound starts.
    pub gain: Scalar,
}
pub static SOUND: NodeType = NodeType {
    type_id: "sound",
    version: 1,
    label: "Sound",
    description: "An audio file (wav, mp3, flac, ogg, m4a, or the sound of a video) on the timeline: timing.start places it, source_in and source_out trim it, and gain sets its level in decibels (read at the sound's start). Every Sound node that is not bypassed plays in Studio's preview and is mixed into video exports with the footage's own sound; it makes no picture and its output connects to nothing.",
    inputs: &[],
    optional: &[],
    output: PortKind::Sound,
    parameters: &[
        other("file", ParameterKind::FilePath, "audio file"),
        other("timing", ParameterKind::MediaTiming, "seconds"),
        scalar("gain", "decibels", -60.0, 12.0, 0.0),
    ],
};
node!(Sound, SOUND, [gain], {
    fn validate(&self) -> Result<()> {
        if self.file.chars().count() > crate::registry::MAX_ASSET_NAME || self.file.contains('\0') {
            return fail("sound needs a file path without NUL, at most 4096 characters");
        }
        self.timing.validate()
    }
});
