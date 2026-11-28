//! Cryptomatte ids: an object's name hashed with MurmurHash3 (x86, 32 bits, seed 0), then turned into a float that is neither infinite, not a number nor denormal, as the Cryptomatte specification has it, so any reader finds the same object by the same name.

/// MurmurHash3, x86, 32 bits, seed 0.
pub fn murmur3(bytes: &[u8]) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
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
    h ^ (h >> 16)
}
/// The Cryptomatte id of a name, as the bits of the float stored in the picture (the
/// document's, which mattes name their objects by).
pub fn id(name: &str) -> u32 {
    veylvik_document::nodes::cryptomatte_id(name)
}
/// The manifest Cryptomatte readers name objects by: `{"name":"hex id",...}`.
pub fn manifest<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let mut seen = std::collections::BTreeMap::new();
    for name in names {
        seen.insert(name.to_owned(), format!("{:08x}", id(name)));
    }
    serde_json::to_string(&seen).unwrap_or_else(|_| "{}".into())
}
/// The short key Cryptomatte metadata is filed under for a layer: the first seven hex
/// digits of the layer name's hash.
pub fn layer_key(layer: &str) -> String {
    format!("{:08x}", murmur3(layer.as_bytes()))[..7].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_hash_as_murmur3_does_everywhere() {
        assert_eq!(murmur3(b""), 0);
        assert_eq!(murmur3(b"hello"), 0x248b_fa47);
        assert_eq!(murmur3(b"Hello, world!"), 0xc036_3e43);
        assert_eq!(
            murmur3(b"The quick brown fox jumps over the lazy dog"),
            0x2e4f_f723
        );
    }
    #[test]
    fn the_documents_ids_are_murmur3_with_the_float_fixed() {
        for name in ["hello", "Red ball", "CryptoObject", "a"] {
            let h = murmur3(name.as_bytes());
            let fixed = if matches!((h >> 23) & 0xff, 0 | 0xff) {
                h ^ (1 << 23)
            } else {
                h
            };
            assert_eq!(id(name), fixed);
        }
    }
    #[test]
    fn an_id_is_always_an_ordinary_float() {
        for name in ["", "a", "Sphere", "Gold title", "x".repeat(40).as_str()] {
            let f = f32::from_bits(id(name));
            assert!(f.is_finite() && (f == 0.0 || f.is_normal()), "{name}: {f}");
        }
        // The empty name hashes to 0: its exponent is 0, so a bit is flipped.
        assert_eq!(id(""), 1 << 23);
    }
}
