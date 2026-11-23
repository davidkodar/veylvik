//! An OpenFX plugin as a node: which plugin, in which context, which of its clips each port feeds, and its settings by the plugin's own names (numbers keyable per channel, or text). The plugin runs outside the renderer, in a helper process.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most settings a node keeps.
pub const OFX_MAX_SETTINGS: usize = 512;
/// The ports, in order; `OfxEffect::clips` names the plugin clip each one feeds.
pub const OFX_PORTS: [&str; 4] = ["source", "input_2", "input_3", "mask"];
/// A setting's value: numbers (each channel keyable) or text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OfxSetting {
    Numbers(Vec<Scalar>),
    Text(String),
}
/// The contexts Veylvik runs a plugin in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OfxContext {
    #[default]
    Filter,
    General,
    Generator,
}
impl OfxContext {
    /// The standard's name for it.
    pub fn name(self) -> &'static str {
        match self {
            OfxContext::Filter => "OfxImageEffectContextFilter",
            OfxContext::General => "OfxImageEffectContextGeneral",
            OfxContext::Generator => "OfxImageEffectContextGenerator",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfxEffect {
    /// The plugin's .ofx.bundle folder (absolute).
    pub bundle: String,
    /// The plugin's identifier in that bundle; empty: none chosen (the source passes on).
    pub plugin: String,
    pub context: OfxContext,
    /// For each port in [`OFX_PORTS`] order, the plugin clip it feeds ("" for none).
    pub clips: Vec<String>,
    /// Settings by the plugin's names; those not here keep the plugin's defaults.
    pub settings: BTreeMap<String, OfxSetting>,
}
impl OfxEffect {
    /// The settings' values at `time`.
    pub fn values_at(&self, time: Time) -> Result<Vec<(String, OfxValue)>> {
        self.settings
            .iter()
            .map(|(name, setting)| {
                let value = match setting {
                    OfxSetting::Numbers(n) => OfxValue::Numbers(
                        n.iter()
                            .map(|s| s.at(time).map(f64::from))
                            .collect::<Result<_>>()?,
                    ),
                    OfxSetting::Text(t) => OfxValue::Text(t.clone()),
                };
                Ok((name.clone(), value))
            })
            .collect()
    }
}
/// A setting evaluated at a moment.
#[derive(Debug, Clone, PartialEq)]
pub enum OfxValue {
    Numbers(Vec<f64>),
    Text(String),
}
pub static OFX_EFFECT: NodeType = NodeType {
    type_id: "ofx_effect",
    version: 1,
    label: "OpenFX effect",
    description: "Runs an OpenFX plugin (an .ofx.bundle, found in OFX_PLUGIN_PATH, /usr/OFX/Plugins or /usr/local/OFX/Plugins) on the CPU in a helper process, so a plugin that crashes or hangs fails this node's render, never the editor. bundle is the bundle's folder and plugin the plugin's identifier in it (list them with `veylvik ofx list`); context is filter (one source), general (several inputs) or generator (no input). clips names, for each port in order (source, input_2, input_3, mask), the plugin clip it feeds (\"\" for none); settings holds values by the plugin's own setting names, numbers as a list of animatable values (one per channel) or text, and settings left out keep the plugin's defaults (see `veylvik ofx describe`). The plugin sees premultiplied scene-linear RGBA float frames of the composition's size and works at full resolution; plugins that need other frames, curve settings, OpenGL or stereo views are not supported yet. With no plugin chosen the source passes on unchanged.",
    inputs: &OFX_PORTS,
    optional: &OFX_PORTS,
    output: PortKind::Image,
    parameters: &[
        other("bundle", ParameterKind::FilePath, ".ofx.bundle folder"),
        other("plugin", ParameterKind::OfxPlugin, "plugin identifier"),
        other(
            "context",
            ParameterKind::OfxContext,
            "filter, general or generator",
        ),
        other("clips", ParameterKind::OfxClips, "plugin clip per port"),
        other(
            "settings",
            ParameterKind::OfxSettings,
            "the plugin's settings",
        ),
    ],
};
node!(OfxEffect, OFX_EFFECT, [], {
    fn validate(&self) -> Result<()> {
        if self.bundle.len() > 4096 || self.plugin.len() > 1024 {
            return crate::fail("an OpenFX effect's bundle or plugin name is too long");
        }
        if self.clips.len() > OFX_PORTS.len() || self.clips.iter().any(|c| c.len() > 256) {
            return crate::fail(
                "an OpenFX effect feeds at most four clips, each named in under 256 characters",
            );
        }
        if self.settings.len() > OFX_MAX_SETTINGS {
            return crate::fail(format!(
                "an OpenFX effect keeps at most {OFX_MAX_SETTINGS} settings"
            ));
        }
        for (name, setting) in &self.settings {
            if name.is_empty() || name.len() > 256 {
                return crate::fail("an OpenFX setting needs a name under 256 characters");
            }
            match setting {
                OfxSetting::Numbers(n) if n.is_empty() || n.len() > 4 => {
                    return crate::fail(format!(
                        "OpenFX setting {name} has {} values (1 to 4)",
                        n.len()
                    ));
                }
                OfxSetting::Numbers(n) => {
                    for s in n {
                        s.validate()?;
                    }
                }
                OfxSetting::Text(t) if t.len() > 65_536 => {
                    return crate::fail(format!("OpenFX setting {name} is too long"));
                }
                OfxSetting::Text(_) => {}
            }
        }
        Ok(())
    }
});
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_effect_keeps_keyed_numbers_and_text_and_refuses_what_is_out_of_bounds() {
        let json = serde_json::json!({
            "type": "ofx_effect", "bundle": "/usr/OFX/Plugins/Misc.ofx.bundle",
            "plugin": "net.sf.openfx.GradePlugin", "context": "filter", "clips": ["Source"],
            "settings": {
                "multiply": [{"kind": "constant", "value": 0.5}, {"kind": "linear", "keys": [
                    {"time": {"ticks": 0, "rate": 24}, "value": 0.0},
                    {"time": {"ticks": 24, "rate": 24}, "value": 1.0}]}],
                "note": "hello"
            }
        });
        let op: Operation = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            json,
            "written back as read"
        );
        let Operation::OfxEffect(effect) = &op else {
            panic!()
        };
        effect.validate().unwrap();
        let half = Time::new(12, 24).unwrap();
        assert_eq!(
            effect.values_at(half).unwrap(),
            vec![
                ("multiply".to_string(), OfxValue::Numbers(vec![0.5, 0.5])),
                ("note".to_string(), OfxValue::Text("hello".into()))
            ]
        );
        let mut wide = effect.clone();
        wide.settings
            .insert("wide".into(), OfxSetting::Numbers(vec![1.0.into(); 5]));
        assert!(wide.validate().is_err());
        let mut many = effect.clone();
        many.clips = vec![String::new(); 5];
        assert!(many.validate().is_err());
    }
}
