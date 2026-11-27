//! Fonts installed on the computer, named by family and style: found through fontconfig, confirmed by the font's own names (never a stand-in), read once and kept for the process.
use crate::{Result, error};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The largest installed font file read (a whole CJK collection can be tens of MB).
const MOST_BYTES: u64 = 64 * 1024 * 1024;

type Found = (Arc<[u8]>, u32);
static KEPT: Mutex<Option<HashMap<(String, String), Found>>> = Mutex::new(None);

/// The bytes and collection index of the installed font `family` in `style`.
pub(crate) fn load(family: &str, style: &str) -> Result<Found> {
    let key = (family.to_owned(), style.to_owned());
    if let Some(found) = KEPT
        .lock()
        .map_err(error)?
        .get_or_insert_with(HashMap::new)
        .get(&key)
    {
        return Ok(found.clone());
    }
    let found = find(family, style)?;
    KEPT.lock()
        .map_err(error)?
        .get_or_insert_with(HashMap::new)
        .insert(key, found.clone());
    Ok(found)
}

fn find(family: &str, style: &str) -> Result<Found> {
    let output = std::process::Command::new("fc-match")
        .arg("--format=%{file}\t%{index}")
        .arg(format!("{family}:style={style}"))
        .output()
        .map_err(|_| error("Installed fonts are found through fontconfig: install fontconfig (fc-match), or embed the font"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let (path, index) = text
        .split_once('\t')
        .and_then(|(p, i)| Some((p.to_owned(), i.trim().parse::<u32>().ok()?)))
        .ok_or_else(|| error(format!("The font {family} {style} is not installed")))?;
    let size = std::fs::metadata(&path).map_err(error)?.len();
    if size > MOST_BYTES {
        return Err(error(format!("The font file {path} is over 64 MiB")));
    }
    let bytes: Arc<[u8]> = Arc::from(std::fs::read(&path).map_err(error)?);
    // fontconfig answers with its nearest font when there is no such one: only the
    // family and style asked for will do.
    let (found_family, found_style) = names(&bytes, index)?;
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    if !same(&found_family, family) || !same(&found_style, style) {
        return Err(error(format!(
            "The font {family} {style} is not installed (the nearest is {found_family} {found_style})"
        )));
    }
    Ok((bytes, index))
}

/// A face's family and style, by its typographic names where it has them.
pub(crate) fn names(bytes: &[u8], index: u32) -> Result<(String, String)> {
    let face = ttf_parser::Face::parse(bytes, index).map_err(|_| error("Not a readable font"))?;
    let name = |ids: &[u16]| {
        ids.iter().find_map(|id| {
            face.names()
                .into_iter()
                .filter(|n| n.name_id == *id)
                .find_map(|n| n.to_string())
        })
    };
    Ok((
        name(&[16, 1]).unwrap_or_default(),
        name(&[17, 2]).unwrap_or_else(|| "Regular".into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_font_that_is_not_installed_is_named_in_the_error_and_never_stood_in_for() {
        // fontconfig answers every request with something: a made-up family must still fail.
        let Err(e) = load("No Such Family Veylvik", "Regular") else {
            panic!("a made-up font was found")
        };
        let words = e.to_string();
        assert!(words.contains("No Such Family Veylvik"), "{words}");
    }
    #[test]
    fn an_installed_font_is_found_by_family_and_style_when_fontconfig_has_it() {
        // Whatever fontconfig picks for "sans", asked for again by its own names, is found.
        let Ok(output) = std::process::Command::new("fc-match")
            .arg("--format=%{file}\t%{index}")
            .arg("sans")
            .output()
        else {
            return; // no fontconfig here: nothing to check
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let Some((path, index)) = text.split_once('\t') else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let index: u32 = index.trim().parse().unwrap();
        let (family, style) = names(&bytes, index).unwrap();
        let (found, at) = load(&family, &style).unwrap();
        assert_eq!(at, index);
        assert_eq!(names(&found, at).unwrap(), (family, style));
    }
}
