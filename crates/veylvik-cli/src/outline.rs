use crate::{Result, assets};
use std::{collections::BTreeSet, path::Path};
use veylvik_document::{Composition, Node, Operation, Time};
pub fn run(args: &[String]) -> Result<()> {
    if args.len() < 3 {
        return Err("outline-text FONT TEXT NEW_FILE [--size N] [--origin X Y] [--canvas W H] [--rtl] [--font-index N] [--language TAG]".into());
    }
    let mut request: assets::text::OutlineRequest =
        serde_json::from_value(serde_json::json!({"font":args[0],"text":args[1]}))?;
    let (mut width, mut height) = (1920, 1080);
    let mut index = 3;
    let mut seen = BTreeSet::new();
    while index < args.len() {
        let flag = args[index].as_str();
        if !seen.insert(flag) {
            return Err(format!("repeated option {flag}").into());
        }
        match flag {
            "--size" if index + 1 < args.len() => {
                request.font_size = args[index + 1].parse()?;
                index += 2;
            }
            "--origin" if index + 2 < args.len() => {
                request.origin = [args[index + 1].parse()?, args[index + 2].parse()?];
                index += 3;
            }
            "--canvas" if index + 2 < args.len() => {
                width = args[index + 1].parse()?;
                height = args[index + 2].parse()?;
                index += 3;
            }
            "--rtl" => {
                request.direction = assets::text::Direction::RightToLeft;
                index += 1;
            }
            "--font-index" if index + 1 < args.len() => {
                request.font_index = args[index + 1].parse()?;
                index += 2;
            }
            "--language" if index + 1 < args.len() => {
                request.language = Some(args[index + 1].clone());
                index += 2;
            }
            _ => return Err(format!("invalid or incomplete outline option {flag}").into()),
        }
    }
    if width == 0
        || height == 0
        || width > veylvik_render::MAX_VECTOR_DIMENSION
        || height > veylvik_render::MAX_VECTOR_DIMENSION
    {
        return Err("outline canvas must be within 1..4096 per axis".into());
    }
    let outlined = assets::text::outline(Path::new("."), &request)?;
    let node = Node::new(
        &request.text,
        Operation::Path {
            geometry: outlined.geometry,
        },
        &[],
    );
    let document = Composition {
        schema: 1,
        width,
        height,
        frame_rate: Time::new(24, 1)?,
        output: node.id,
        nodes: [(node.id, node)].into(),
    };
    document.validate()?;
    // A conversion always creates a new file: never replace the font or an
    // existing composition, including symlinks and concurrent creations.
    veylvik_document::atomic_create(Path::new(&args[2]), &serde_json::to_vec_pretty(&document)?)?;
    println!(
        "{}",
        serde_json::json!({"saved":args[2],"glyph_count":outlined.glyph_count,"advance":outlined.advance,"width":width,"height":height})
    );
    Ok(())
}
