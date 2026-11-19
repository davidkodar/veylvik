//! Readable names for settings: a registry parameter id and its unit become what a person reads on a slider or a track.

/// `clip_black` with unit `amount` reads "Clip black"; `shrink` with `pixels` reads
/// "Shrink (px)". A few ids whose plain form says too little get their own words.
pub fn setting(name: &str, unit: &str) -> String {
    let words = match name {
        "darks" => "Keep darks".to_owned(),
        "f_stop" => "F-stop".to_owned(),
        "x" | "y" | "z" => name.to_ascii_uppercase(),
        _ => {
            let spaced = name.replace('_', " ");
            let mut letters = spaced.chars();
            match letters.next() {
                Some(first) => first.to_uppercase().chain(letters).collect(),
                None => spaced,
            }
        }
    };
    // Units that say something are kept short; "amount" and "factor" say nothing.
    let short = match unit {
        "" | "amount" | "factor" | "gain" | "contrast" | "offset" | "index" | "count" => "",
        "pixels" => "px",
        "degrees" => "°",
        "seconds" => "s",
        "frames" => "fr",
        "decibels" => "dB",
        "millimetres" => "mm",
        "pixels per second" => "px/s",
        "pixels per second squared" => "px/s²",
        "degrees per second" => "°/s",
        "per second" => "/s",
        other => other,
    };
    if short.is_empty() {
        words
    } else {
        format!("{words} ({short})")
    }
}
#[cfg(test)]
mod tests {
    use super::setting;
    #[test]
    fn ids_read_as_words_and_only_telling_units_are_kept() {
        assert_eq!(setting("clip_black", "amount"), "Clip black");
        assert_eq!(setting("darks", "amount"), "Keep darks");
        assert_eq!(setting("shrink", "pixels"), "Shrink (px)");
        assert_eq!(setting("rotation", "degrees"), "Rotation (°)");
        assert_eq!(setting("x", "pixels"), "X (px)");
        assert_eq!(setting("exposure", "stops"), "Exposure (stops)");
    }
}
