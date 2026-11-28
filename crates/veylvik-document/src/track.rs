//! Animated colours and 3D vectors: one key holds all three channels, so a colour or a
//! position is keyed as a whole. A constant stays a plain `[x, y, z]` array in JSON, so
//! values written before tracks existed load unchanged.
use crate::{Interpolation, Key, Result, Scalar, Time, animation::MAX_KEYS, fail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Track3 {
    Constant([f32; 3]),
    Animated(Curve3),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve3 {
    pub interpolation: Curve3Mode,
    #[serde(with = "crate::animation::even")]
    pub keys: Vec<Key3>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve3Mode {
    Linear,
    Step,
    Smooth,
    Monotone,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key3 {
    pub time: Time,
    pub value: [f32; 3],
}
impl PartialEq for Key3 {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.time.compare(other.time).is_ok_and(|o| o.is_eq())
    }
}
impl From<[f32; 3]> for Track3 {
    fn from(value: [f32; 3]) -> Self {
        Self::Constant(value)
    }
}
impl Default for Track3 {
    fn default() -> Self {
        Self::Constant([0.0; 3])
    }
}
impl From<Interpolation> for Curve3Mode {
    fn from(mode: Interpolation) -> Self {
        match mode {
            Interpolation::Linear => Self::Linear,
            Interpolation::Step => Self::Step,
            Interpolation::Smooth => Self::Smooth,
            Interpolation::Monotone => Self::Monotone,
        }
    }
}
impl From<Curve3Mode> for Interpolation {
    fn from(mode: Curve3Mode) -> Self {
        match mode {
            Curve3Mode::Linear => Self::Linear,
            Curve3Mode::Step => Self::Step,
            Curve3Mode::Smooth => Self::Smooth,
            Curve3Mode::Monotone => Self::Monotone,
        }
    }
}
impl Track3 {
    /// The value when it does not change over time.
    pub fn constant(&self) -> Option<[f32; 3]> {
        match self {
            Self::Constant(value) => Some(*value),
            Self::Animated(_) => None,
        }
    }
    pub fn is_animated(&self) -> bool {
        matches!(self, Self::Animated(_))
    }
    pub fn keys(&self) -> &[Key3] {
        match self {
            Self::Constant(_) => &[],
            Self::Animated(curve) => &curve.keys,
        }
    }
    pub fn interpolation(&self) -> Option<Interpolation> {
        match self {
            Self::Constant(_) => None,
            Self::Animated(curve) => Some(curve.interpolation.into()),
        }
    }
    /// Each channel as a scalar curve with the same keys and interpolation.
    pub fn channel(&self, channel: usize) -> Scalar {
        match self {
            Self::Constant(value) => value[channel].into(),
            Self::Animated(curve) => {
                let keys = curve
                    .keys
                    .iter()
                    .map(|key| Key {
                        segment: None,
                        time: key.time,
                        value: key.value[channel],
                    })
                    .collect();
                match curve.interpolation {
                    Curve3Mode::Linear => Scalar::Linear { keys },
                    Curve3Mode::Step => Scalar::Step { keys },
                    Curve3Mode::Smooth => Scalar::Smooth { keys },
                    Curve3Mode::Monotone => Scalar::Monotone { keys },
                }
            }
        }
    }
    pub fn at(&self, time: Time) -> Result<[f32; 3]> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Animated(_) => Ok([
                self.channel(0).at(time)?,
                self.channel(1).at(time)?,
                self.channel(2).at(time)?,
            ]),
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Constant(value) if value.iter().all(|v| v.is_finite()) => Ok(()),
            Self::Constant(_) => fail("non-finite colour or vector"),
            Self::Animated(curve) => {
                if curve.keys.is_empty() || curve.keys.len() > MAX_KEYS {
                    return fail(format!("animation needs 1..={MAX_KEYS} keys"));
                }
                (0..3).try_for_each(|c| self.channel(c).validate())
            }
        }
    }
    /// Insert or update a key at an exact time. A constant becomes a one-key linear
    /// track. Failure leaves the track unchanged.
    pub fn set_key_at(&mut self, time: Time, value: [f32; 3]) -> Result<()> {
        time.seconds()?;
        if !value.iter().all(|v| v.is_finite()) {
            return fail("non-finite key");
        }
        let mut candidate = self.clone();
        match &mut candidate {
            Self::Constant(_) => {
                candidate = Self::Animated(Curve3 {
                    interpolation: Curve3Mode::Linear,
                    keys: vec![Key3 { time, value }],
                })
            }
            Self::Animated(curve) => {
                let mut position = Err(curve.keys.len());
                for (i, key) in curve.keys.iter().enumerate() {
                    match key.time.compare(time)? {
                        std::cmp::Ordering::Less => {}
                        std::cmp::Ordering::Equal => {
                            position = Ok(i);
                            break;
                        }
                        std::cmp::Ordering::Greater => {
                            position = Err(i);
                            break;
                        }
                    }
                }
                match position {
                    Ok(i) => curve.keys[i].value = value,
                    Err(_) if curve.keys.len() == MAX_KEYS => {
                        return fail(format!("animation exceeds {MAX_KEYS} keys"));
                    }
                    Err(i) => curve.keys.insert(i, Key3 { time, value }),
                }
            }
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Remove the key at an exact time; the last key's value stays as a constant.
    pub fn remove_key_at(&mut self, time: Time) -> Result<()> {
        let Self::Animated(curve) = self else {
            return fail("track has no keyframes");
        };
        let mut index = None;
        for (i, key) in curve.keys.iter().enumerate() {
            if key.time.compare(time)?.is_eq() {
                index = Some(i);
            }
        }
        let Some(index) = index else {
            return fail("no keyframe at requested time");
        };
        let mut keys = curve.keys.clone();
        let removed = keys.remove(index);
        *self = if keys.is_empty() {
            Self::Constant(removed.value)
        } else {
            Self::Animated(Curve3 {
                interpolation: curve.interpolation,
                keys,
            })
        };
        Ok(())
    }
    /// The keys at `indices` moved by `offset` (or copies of them, when `duplicate`);
    /// a moved key replaces one already at its new time.
    pub fn shifted(
        &self,
        indices: &std::collections::BTreeSet<usize>,
        offset: Time,
        duplicate: bool,
    ) -> Result<Self> {
        let Self::Animated(curve) = self else {
            return fail("track has no keyframes");
        };
        let mut moved = Vec::new();
        for &i in indices {
            let key = curve
                .keys
                .get(i)
                .ok_or_else(|| crate::Error("no such key".into()))?;
            moved.push(Key3 {
                time: key.time.checked_add(offset)?,
                value: key.value,
            });
        }
        let mut keys: Vec<Key3> = curve
            .keys
            .iter()
            .enumerate()
            .filter(|(i, _)| duplicate || !indices.contains(i))
            .map(|(_, k)| *k)
            .filter(|k| {
                !moved
                    .iter()
                    .any(|m| m.time.compare(k.time).is_ok_and(|o| o.is_eq()))
            })
            .collect();
        keys.extend(moved);
        let mut order = Vec::with_capacity(keys.len());
        for key in keys {
            let at = order
                .iter()
                .position(|o: &Key3| o.time.compare(key.time).is_ok_and(|c| c.is_gt()))
                .unwrap_or(order.len());
            order.insert(at, key);
        }
        let track = Self::Animated(Curve3 {
            interpolation: curve.interpolation,
            keys: order,
        });
        track.validate()?;
        Ok(track)
    }
    /// Without the keys at `indices`; removing every key keeps the value at `now`.
    pub fn without(&self, indices: &std::collections::BTreeSet<usize>, now: Time) -> Result<Self> {
        let Self::Animated(curve) = self else {
            return fail("track has no keyframes");
        };
        let keys: Vec<Key3> = curve
            .keys
            .iter()
            .enumerate()
            .filter(|(i, _)| !indices.contains(i))
            .map(|(_, k)| *k)
            .collect();
        Ok(if keys.is_empty() {
            Self::Constant(self.at(now)?)
        } else {
            Self::Animated(Curve3 {
                interpolation: curve.interpolation,
                keys,
            })
        })
    }
    /// Change how keys blend; a constant stays a constant.
    pub fn set_interpolation(&mut self, mode: Interpolation) {
        if let Self::Animated(curve) = self {
            curve.interpolation = mode.into();
        }
    }
    /// Edit the value shown at `time`: the key there when animated (adding one if
    /// needed), else the constant.
    pub fn set_at(&mut self, time: Time, value: [f32; 3]) -> Result<()> {
        if self.is_animated() {
            self.set_key_at(time, value)
        } else if value.iter().all(|v| v.is_finite()) {
            *self = Self::Constant(value);
            Ok(())
        } else {
            fail("non-finite colour or vector")
        }
    }
}

/// A number that may be keyed; plain JSON numbers (written before it could be keyed)
/// load as constants.
pub fn number_or_scalar<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Scalar, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Number(f32),
        Scalar(Scalar),
    }
    Ok(match Either::deserialize(deserializer)? {
        Either::Number(value) => value.into(),
        Either::Scalar(scalar) => scalar,
    })
}
/// Key or set a number at `time`: a key when it is animated, else the constant.
pub fn set_scalar_at(scalar: &mut Scalar, time: Time, value: f32) -> Result<()> {
    if scalar.keys().is_some() {
        scalar.set_key_at(time, value)
    } else if value.is_finite() {
        *scalar = value.into();
        Ok(())
    } else {
        fail("non-finite value")
    }
}

/// A keyable property inside a node's structured values, addressed by a path:
/// `colours.0`..`colours.2` on a Text surface; `studio.lights.N.position`,
/// `.rotation`, `.colour` or `.intensity` on a Text surface or Light rig; and
/// `placement.position`, `.rotation`, `.scale` and `colour` on 3D scene nodes.
pub enum Property<'a> {
    Vector(&'a mut Track3),
    Number(&'a mut Scalar),
}
impl crate::Operation {
    /// Every keyed colour, vector or light intensity with its key times.
    pub fn keyed_properties(&self) -> Vec<(String, Vec<Time>)> {
        let mut found = Vec::new();
        let mut scene = self.clone();
        if let (Some(placement), colour) = scene.scene_parts_mut() {
            for (path, track) in [
                ("placement.position", &placement.position),
                ("placement.rotation", &placement.rotation),
                ("placement.scale", &placement.scale),
            ]
            .into_iter()
            .chain(colour.map(|c| ("colour", &*c)))
            {
                if track.is_animated() {
                    found.push((
                        path.to_owned(),
                        track.keys().iter().map(|k| k.time).collect(),
                    ));
                }
            }
            return found;
        }
        let studio = match self {
            crate::Operation::Material(m) => {
                for (i, colour) in m.colours.iter().flatten().enumerate() {
                    if colour.is_animated() {
                        found.push((
                            format!("colours.{i}"),
                            colour.keys().iter().map(|k| k.time).collect(),
                        ));
                    }
                }
                &m.studio
            }
            crate::Operation::LightRig(rig) => &rig.studio,
            _ => return found,
        };
        for (n, light) in studio.lights.iter().enumerate() {
            for (field, track) in [
                ("position", &light.position),
                ("rotation", &light.rotation),
                ("colour", &light.colour),
            ] {
                if track.is_animated() {
                    found.push((
                        format!("studio.lights.{n}.{field}"),
                        track.keys().iter().map(|k| k.time).collect(),
                    ));
                }
            }
            if let Some(keys) = light.intensity.keys() {
                found.push((
                    format!("studio.lights.{n}.intensity"),
                    keys.iter().map(|k| k.time).collect(),
                ));
            }
        }
        found
    }
    fn is_scene_node(&self) -> bool {
        use crate::Operation as O;
        matches!(
            self,
            O::Shape3d(_)
                | O::Text3d(_)
                | O::Light3d(_)
                | O::ImageCard(_)
                | O::Transform3d(_)
                | O::Camera3d(_)
                | O::Figure3d(_)
                | O::Marker3d(_)
                | O::Volume3d(_)
        )
    }
    /// Placement and colour of 3D scene nodes.
    fn scene_parts_mut(&mut self) -> (Option<&mut crate::nodes::Placement>, Option<&mut Track3>) {
        use crate::Operation as O;
        match self {
            O::Shape3d(n) => (Some(&mut n.placement), Some(&mut n.colour)),
            O::Text3d(n) => (Some(&mut n.placement), Some(&mut n.colour)),
            O::Light3d(n) => (Some(&mut n.placement), Some(&mut n.colour)),
            O::ImageCard(n) => (Some(&mut n.placement), Some(&mut n.colour)),
            O::Transform3d(n) => (Some(&mut n.placement), None),
            O::Camera3d(n) => (Some(&mut n.placement), None),
            O::Figure3d(n) => (Some(&mut n.placement), Some(&mut n.colour)),
            O::Marker3d(n) => (Some(&mut n.placement), None),
            O::Volume3d(n) => (Some(&mut n.placement), Some(&mut n.colour)),
            _ => (None, None),
        }
    }
    pub fn property_mut(&mut self, path: &str) -> Option<Property<'_>> {
        let parts: Vec<&str> = path.split('.').collect();
        if self.is_scene_node() {
            let (Some(placement), colour) = self.scene_parts_mut() else {
                return None;
            };
            return match parts[..] {
                ["placement", "position"] => Some(Property::Vector(&mut placement.position)),
                ["placement", "rotation"] => Some(Property::Vector(&mut placement.rotation)),
                ["placement", "scale"] => Some(Property::Vector(&mut placement.scale)),
                ["colour"] => colour.map(Property::Vector),
                _ => None,
            };
        }
        let studio = match self {
            crate::Operation::Material(m) => {
                if let ["colours", index] = parts[..] {
                    let index: usize = index.parse().ok().filter(|i| *i < 3)?;
                    // Keying a family's own colours starts from them.
                    let palette = m
                        .colours
                        .get_or_insert_with(|| crate::optical::palette_track(m.kind.palette()));
                    return Some(Property::Vector(&mut palette[index]));
                }
                &mut m.studio
            }
            crate::Operation::LightRig(rig) => &mut rig.studio,
            _ => return None,
        };
        let ["studio", "lights", index, field] = parts[..] else {
            return None;
        };
        let light = studio.lights.get_mut(index.parse::<usize>().ok()?)?;
        Some(match field {
            "position" => Property::Vector(&mut light.position),
            "rotation" => Property::Vector(&mut light.rotation),
            "colour" => Property::Vector(&mut light.colour),
            "intensity" => Property::Number(&mut light.intensity),
            _ => return None,
        })
    }
}

/// Every value a track takes at its keys (or its constant).
pub fn values(track: &Track3) -> impl Iterator<Item = [f32; 3]> + '_ {
    let constant = track.constant();
    constant
        .into_iter()
        .chain(track.keys().iter().map(|k| k.value))
}
/// JSON schema of a track whose channels stay within `lo..=hi`.
pub fn schema(lo: f32, hi: f32) -> serde_json::Value {
    let vector = serde_json::json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":lo,"maximum":hi}});
    serde_json::json!({"oneOf":[vector,{"type":"object","additionalProperties":false,"required":["interpolation","keys"],"properties":{
        "interpolation":{"enum":["linear","step","smooth","monotone"]},
        "keys":{"anyOf":[{"type":"array","minItems":1,"maxItems":crate::animation::MAX_KEYS,"items":{"type":"object","additionalProperties":false,
            "required":["time","value"],"properties":{"time":crate::registry::time_schema(),"value":vector}}},crate::registry::even_schema(vector.clone())]}}}]})
}

impl crate::animation::even::Keyed for Key3 {
    type Value = [f32; 3];
    fn time(&self) -> Time {
        self.time
    }
    fn value(&self) -> [f32; 3] {
        self.value
    }
    fn plain(&self) -> bool {
        true
    }
    fn at(time: Time, value: [f32; 3]) -> Self {
        Key3 { time, value }
    }
}
