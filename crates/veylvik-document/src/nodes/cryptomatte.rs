//! A Cryptomatte matte: the objects picked by name out of a Render scene's objects passes (or a file's Cryptomatte layers), as a soft-edged matte with each object's exact share of every pixel.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// The most objects one matte picks.
pub const MAX_CRYPTO_OBJECTS: usize = 256;

/// The matte of the picked objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cryptomatte {
    /// The objects in the matte: each a name (a 3D object's node label), or `#` and its
    /// Cryptomatte id as eight hex digits when it has no name here.
    #[serde(default)]
    pub objects: Vec<String>,
}
impl Cryptomatte {
    /// The ids of `objects` (names hashed as Cryptomatte does; `#` entries as given).
    pub fn ids(&self) -> Vec<u32> {
        self.objects.iter().filter_map(|o| object_id(o)).collect()
    }
}
/// The Cryptomatte id an entry of `objects` stands for.
pub fn object_id(entry: &str) -> Option<u32> {
    match entry.strip_prefix('#') {
        Some(hex) if hex.len() == 8 => u32::from_str_radix(hex, 16).ok(),
        Some(_) => None,
        None => Some(cryptomatte_id(entry)),
    }
}
/// Cryptomatte's id of a name: MurmurHash3 (x86, 32 bits, seed 0), with the float's
/// exponent kept away from 0 and 255 so the id is an ordinary float.
pub fn cryptomatte_id(name: &str) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    let bytes = name.as_bytes();
    let mut h: u32 = 0;
    let (blocks, tail) = bytes.as_chunks::<4>();
    for block in blocks {
        let k = u32::from_le_bytes(*block)
            .wrapping_mul(C1)
            .rotate_left(15)
            .wrapping_mul(C2);
        h = (h ^ k)
            .rotate_left(13)
            .wrapping_mul(5)
            .wrapping_add(0xe654_6b64);
    }
    if !tail.is_empty() {
        let mut k: u32 = 0;
        for (i, b) in tail.iter().enumerate() {
            k |= u32::from(*b) << (8 * i);
        }
        h ^= k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
    }
    h ^= bytes.len() as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    let exponent = (h >> 23) & 0xff;
    if exponent == 0 || exponent == 0xff {
        h ^ (1 << 23)
    } else {
        h
    }
}

pub static CRYPTOMATTE: NodeType = NodeType {
    type_id: "cryptomatte",
    version: 1,
    label: "Cryptomatte",
    description: "A matte of the objects picked out of a Render scene's objects pass (Pass objects) on Objects, and its objects_more pass on Objects more if connected: white where the picked objects are, each pixel as much as they cover it, with soft edges exactly as rendered. Objects lists them by name (the 3D object's node label) or as # and the id in eight hex digits. In Studio, Pick and click in the viewer to add or remove the object there. Connect it to a Mask node's mask to cut those objects out, or keep the rest. Outputs a white picture with the matte as alpha.",
    inputs: &["objects", "objects_more"],
    optional: &["objects_more"],
    output: PortKind::Image,
    parameters: &[other(
        "objects",
        ParameterKind::CryptoObjects,
        "names or #ids",
    )],
};

node!(Cryptomatte, CRYPTOMATTE, [], {
    fn validate(&self) -> Result<()> {
        if self.objects.len() > MAX_CRYPTO_OBJECTS {
            return fail(format!(
                "Cryptomatte picks at most {MAX_CRYPTO_OBJECTS} objects"
            ));
        }
        for entry in &self.objects {
            if entry.is_empty() || entry.len() > 256 || object_id(entry).is_none() {
                return fail(format!(
                    "Cryptomatte object {entry:?} is neither a name (1 to 256 bytes) nor # and eight hex digits"
                ));
            }
        }
        Ok(())
    }
});

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_ids_stand_for_the_same_object() {
        let id = cryptomatte_id("Red ball");
        let matte = Cryptomatte {
            objects: vec!["Red ball".into(), format!("#{id:08x}"), "#12".into()],
        };
        assert_eq!(matte.ids(), vec![id, id]);
        assert_eq!(cryptomatte_id(""), 1 << 23);
        assert!(super::super::NodeOp::validate(&matte).is_err());
    }
}
