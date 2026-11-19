//! Sounds that ship with Veylvik, for effects that bring their own: a Sound node names one as `builtin:NAME`, and it is written once to the user's cache, where every reader of sound finds it as an ordinary file.
use crate::Result;
use std::path::{Path, PathBuf};

/// What a Sound node's file starts with when it names a bundled sound.
pub const PREFIX: &str = "builtin:";
/// The bundled sounds: name and WAV data.
const ALL: &[(&str, &[u8])] = &[(
    "switch_off",
    include_bytes!("../assets/sounds/switch_off.wav"),
)];

/// The largest sound an effect may bring with it. A longer or larger sound is
/// added by the user as a Sound node of their own, never pulled in by an effect.
pub const MAX_EFFECT_SOUND_BYTES: u64 = 4 * 1024 * 1024;
/// Checks a sound an effect brings: a bundled one that exists, or an audio file
/// (by its extension, see `export_audio::is_audio`) of at most 4 MiB beside `base`.
pub fn check_effect_sound(base: &Path, file: &str) -> Result<()> {
    if let Some(name) = file.strip_prefix(PREFIX) {
        return if names().any(|known| known == name) {
            Ok(())
        } else {
            Err(format!("Veylvik has no bundled sound called {name}").into())
        };
    }
    let path = base.join(file);
    if !crate::export_audio::is_audio(&path) {
        return Err(format!(
            "{file}: an effect's sound must be wav, mp3, flac, ogg, opus, m4a, aac or aiff"
        )
        .into());
    }
    let size = std::fs::metadata(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size > MAX_EFFECT_SOUND_BYTES {
        return Err(format!(
            "{file} is {} MiB: an effect brings sounds of at most 4 MiB; add a longer sound as a Sound node of its own",
            size.div_ceil(1024 * 1024)
        )
        .into());
    }
    Ok(())
}
/// The names of the bundled sounds.
pub fn names() -> impl Iterator<Item = &'static str> {
    ALL.iter().map(|(name, _)| *name)
}
/// Where bundled sounds are written: `$XDG_CACHE_HOME/veylvik/sounds`, or under
/// `~/.cache`.
fn directory() -> Result<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or("no cache folder: set XDG_CACHE_HOME or HOME")?;
    Ok(cache.join("veylvik").join("sounds"))
}
/// The file a Sound node names: a path under the project's folder, or a bundled sound,
/// written to the cache if it is not there already (complete, or not at all).
pub fn resolve(base: &Path, file: &str) -> Result<PathBuf> {
    let Some(name) = file.strip_prefix(PREFIX) else {
        return Ok(base.join(file));
    };
    let (_, data) = ALL
        .iter()
        .find(|(known, _)| *known == name)
        .ok_or_else(|| format!("Veylvik has no bundled sound called {name}"))?;
    let directory = directory()?;
    // The length in the name: a later version's sound never reuses an older file.
    let path = directory.join(format!("{name}-{}.wav", data.len()));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == data.len() as u64) {
        return Ok(path);
    }
    std::fs::create_dir_all(&directory)?;
    let partial = directory.join(format!(".{name}-{}.part", std::process::id()));
    std::fs::write(&partial, data)?;
    std::fs::rename(&partial, &path)?;
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_project_path_is_joined_and_an_unknown_bundled_sound_is_refused() {
        assert_eq!(
            resolve(Path::new("/project"), "sound/hit.wav").unwrap(),
            PathBuf::from("/project/sound/hit.wav")
        );
        assert!(resolve(Path::new("/project"), "builtin:nothing").is_err());
        assert!(names().any(|name| name == "switch_off"));
        // The bundled sound is a WAV file.
        assert!(ALL.iter().all(|(_, data)| data.starts_with(b"RIFF")));
        // What an effect may bring: a bundled sound, or a small audio file.
        check_effect_sound(Path::new("/nowhere"), "builtin:switch_off").unwrap();
        assert!(check_effect_sound(Path::new("/nowhere"), "builtin:nothing").is_err());
        assert!(check_effect_sound(Path::new("/nowhere"), "notes.txt").is_err());
        let dir = std::env::temp_dir().join(format!("veylvik-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("small.wav"), [0_u8; 64]).unwrap();
        check_effect_sound(&dir, "small.wav").unwrap();
        let big = std::fs::File::create(dir.join("big.wav")).unwrap();
        big.set_len(MAX_EFFECT_SOUND_BYTES + 1).unwrap();
        let refused = check_effect_sound(&dir, "big.wav").unwrap_err().to_string();
        assert!(refused.contains("at most 4 MiB"), "{refused}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
