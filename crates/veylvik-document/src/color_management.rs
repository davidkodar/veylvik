//! Explicit OCIO transforms. Existing projects retain linear Rec.709 by default.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcioTransform {
    /// Built-in OCIO URI or project-relative configuration file.
    pub config: String,
    pub source: String,
    pub destination: String,
    /// When set, apply the display/view, then convert its display colour space
    /// to destination. This avoids double display encoding in the standard viewer.
    pub display: String,
    pub view: String,
}
impl Default for OcioTransform {
    fn default() -> Self {
        Self {
            config: "ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5".into(),
            source: "Linear Rec.709 (sRGB)".into(),
            destination: "Linear Rec.709 (sRGB)".into(),
            display: String::new(),
            view: String::new(),
        }
    }
}
impl OcioTransform {
    pub fn validate(&self) -> crate::Result<()> {
        for value in [
            &self.config,
            &self.source,
            &self.destination,
            &self.display,
            &self.view,
        ] {
            if value.len() > 4096 || value.contains('\0') {
                return crate::fail("OCIO names must be at most 4096 bytes without NUL");
            }
        }
        if self.config.trim().is_empty()
            || self.source.trim().is_empty()
            || (self.display.is_empty() && self.destination.is_empty())
            || (!self.display.is_empty() && self.view.is_empty())
        {
            return crate::fail(
                "OCIO needs a configuration, source and destination or display/view",
            );
        }
        Ok(())
    }
    pub fn schema() -> serde_json::Value {
        let name = serde_json::json!({"type":"string","maxLength":4096});
        serde_json::json!({"type":"object","properties":{"config":name,"source":name,"destination":name,"display":name,"view":name},"required":["config","source","destination","display","view"],"additionalProperties":false})
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSpace {
    #[default]
    LinearRec709,
    Acescg,
}
impl WorkingSpace {
    pub fn is_default(&self) -> bool {
        *self == Self::LinearRec709
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcioViews {
    pub view: Option<OcioTransform>,
    pub output: Option<OcioTransform>,
}
impl OcioViews {
    pub fn schema() -> serde_json::Value {
        let transform = serde_json::json!({"anyOf":[{"type":"null"},OcioTransform::schema()]});
        serde_json::json!({"type":"object","properties":{"view":transform,"output":transform},"required":["view","output"],"additionalProperties":false})
    }

    pub fn is_default(&self) -> bool {
        self.view.is_none() && self.output.is_none()
    }
    pub fn validate(&self) -> crate::Result<()> {
        for transform in [&self.view, &self.output].into_iter().flatten() {
            transform.validate()?;
        }
        Ok(())
    }
}
