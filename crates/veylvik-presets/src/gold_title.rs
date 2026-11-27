//! Gold title: brushed gold 3D lettering under studio strip lights, on a dark wall with a warm beam. Every measure scales with the composition's width.
use crate::{Built, Scene, c};
use serde_json::json;
use veylvik_document::{Command, Composition, Result, media::DisplayTransform, nodes::*};

/// The scene's nodes for `composition`, with `text` as the title and `subtitle`
/// under it (empty for none). The display transform becomes Filmic, which the look
/// relies on, and the scene becomes the output.
pub fn gold_title(composition: &Composition, text: &str, subtitle: &str) -> Result<Built> {
    gold_title_with(composition, text, subtitle, true)
}
/// As [`gold_title`]; `shadow` adds the letters' shadow on the wall behind them (a wall
/// plane that only catches shadows, and the soft light that casts them). Both are nodes
/// of their own, so the shadow can also be faded or removed afterwards.
pub fn gold_title_with(
    composition: &Composition,
    text: &str,
    subtitle: &str,
    shadow: bool,
) -> Result<Built> {
    let (w, h) = (composition.width as f32, composition.height as f32);
    let s = w / 1920.0;
    let (cx, cy) = (w * 0.5, h * 0.5);
    let mut scene = Scene::below(composition);
    // Sized so the title fits the frame with a margin: capitals in the built-in
    // bold font are about 0.6 of their size wide on average (an estimate; the font
    // browser's fonts differ, and the size is the Title node's to change).
    let longest = text
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(1)
        .max(1);
    let size = (0.82 * 1920.0 / (0.6 * longest as f32)).clamp(60.0, 340.0);
    let title = scene.add(
        "Title",
        &TEXT,
        json!({"content": text, "size": c(size * s), "x": c(cx), "y": c(cy + 70.0 * s)}),
        &[],
    )?;
    let lettering = scene.add(
        "Gold lettering",
        &TEXT_3D,
        json!({
            "depth": c(40.0 * s), "bevel": c(6.0 * s), "round_bevel": true,
            "colour": [1.0, 0.57, 0.14], "roughness": c(0.15), "metallic": c(1.0),
            "brushed": c(0.6), "brush_angle": c(65.0), "grain": c(0.37),
            "ridges": c(0.12), "imperfections": c(0.12),
        }),
        &[("outline", title)],
    )?;
    let camera = scene.add("Camera", &CAMERA_3D, json!({"f_stop": c(22.0)}), &[])?;
    // A dim, even room: the metal's dark base. The lights do the rest.
    let room = scene.add(
        "Room tone",
        &SOLID,
        json!({"color": [0.1, 0.072, 0.046, 1.0]}),
        &[],
    )?;
    let environment = scene.add("Room", &ENVIRONMENT_3D, json!({}), &[("image", room)])?;
    let mut parts = vec![lettering, camera, environment];
    let light = |position: [f32; 3], rotation: [f32; 3]| json!({"position": position.map(|v| v * s), "rotation": rotation, "scale": [1.0, 1.0, 1.0]});
    // Three rows of uneven strip lights in front of the sign: the bands of light.
    for (i, (y, intensity, height)) in [
        (-185.0, 0.87, 15.6),
        (-62.0, 1.24, 13.9),
        (188.0, 1.32, 13.4),
    ]
    .into_iter()
    .enumerate()
    {
        parts.push(scene.add(
            &format!("Strip lights {}", i + 1),
            &LIGHT_3D,
            json!({
                "kind": "area", "shadows": false, "colour": [1.0, 0.84, 0.58],
                "placement": light([0.0, y, 400.0], [0.0, 0.0, -14.0]),
                "intensity": c(intensity), "width": c(1700.0 * s), "height": c(height * s),
                "strips": c(4.0), "variation": c(0.8), "seed": 3 + i,
            }),
            &[],
        )?);
    }
    // Small bright panels: the white-hot cores inside the bands.
    for (i, (x, y)) in [
        (-470.0, -45.0),
        (-300.0, 5.0),
        (-130.0, -80.0),
        (60.0, 10.0),
        (215.0, 60.0),
        (330.0, -85.0),
        (520.0, -30.0),
    ]
    .into_iter()
    .enumerate()
    {
        parts.push(scene.add(
            &format!("Hot spot {}", i + 1),
            &LIGHT_3D,
            json!({
                "kind": "area", "shadows": false, "colour": [1.0, 0.88, 0.66],
                "placement": light([x, y, 400.0], [0.0, 0.0, -14.0]),
                "intensity": c(7.8), "width": c(90.0 * s), "height": c(13.0 * s),
            }),
            &[],
        )?);
    }
    // Point lights close to the sign: stars on its corners.
    for (i, (x, y, intensity)) in [
        (-527.0, -45.0, 0.0045),
        (-184.0, -95.0, 0.003),
        (180.0, 15.0, 0.0055),
        (375.0, -100.0, 0.003),
        (644.0, 105.0, 0.004),
    ]
    .into_iter()
    .enumerate()
    {
        parts.push(scene.add(
            &format!("Star {}", i + 1),
            &LIGHT_3D,
            json!({
                "kind": "point", "shadows": false, "colour": [1.0, 0.9, 0.72],
                // Point lights fall off with the square of the distance.
                "placement": light([x, y, 50.0], [0.0; 3]), "intensity": c(intensity * s * s),
            }),
            &[],
        )?);
    }
    // Two glancing key lights, from above and from the left: they catch the bevels
    // facing them and leave the faces alone, which gives the letters depth. In reflections
    // they show at 0.3: a sun is a point of light, and at full strength its reflection runs
    // along a round bevel as a bright line (path traced, the true reflection shows).
    for (label, rotation) in [
        ("Key from above", [65.0, 0.0, 0.0]),
        ("Key from the left", [0.0, -65.0, 0.0]),
    ] {
        parts.push(scene.add(
            label,
            &LIGHT_3D,
            json!({
                "kind": "sun", "shadows": false, "colour": [1.0, 0.93, 0.8],
                "placement": light([0.0, 0.0, 1000.0], rotation), "intensity": c(1.7),
                "reflections": c(0.3),
            }),
            &[],
        )?);
    }
    if shadow {
        // The wall the letters stand in front of, as something their shadow falls on:
        // it shows only the shadow, over the painted wall behind the scene.
        parts.push(scene.add(
            "Shadow on the wall",
            &SHAPE_3D,
            json!({
                "kind": "plane", "shadow_catcher": true, "shadow_strength": c(8.0),
                "size_x": c(w * 2.4), "size_y": c(h * 2.4),
                "placement": light([0.0, 0.0, -50.0], [0.0; 3]),
            }),
            &[],
        )?);
        // A modest light from the upper left front, to throw that shadow down and to the
        // right. The scene's many other lights wash a shadow out, so the wall's Shadow
        // strength is raised well above 1 to keep it.
        parts.push(scene.add(
            "Shadow light",
            &LIGHT_3D,
            json!({
                "kind": "sun", "shadows": true, "colour": [1.0, 0.93, 0.8],
                "placement": light([0.0, 0.0, 1000.0], [22.0, -24.0, 0.0]), "intensity": c(0.5),
                "reflections": c(0.3),
            }),
            &[],
        )?);
    }
    let merged = scene.merge("Gold title scene", parts)?;
    let render = scene.add(
        "Render gold title",
        &RENDER_SCENE,
        json!({}),
        &[("scene", merged)],
    )?;
    // The wall: near black, with a warm beam from lower left to upper right.
    let wall = scene.add(
        "Wall",
        &SOLID,
        json!({"color": [0.024, 0.023, 0.025, 1.0]}),
        &[],
    )?;
    let noise = scene.add(
        "Haze noise",
        &FRACTAL_NOISE,
        json!({"seed": 5, "scale": c(120.0 * s), "octaves": c(3.0), "roughness": c(0.5)}),
        &[],
    )?;
    let streaks = scene.add(
        "Haze streaks",
        &AFFINE,
        json!({"pivot_x": c(cx), "pivot_y": c(cy), "rotation": c(62.0), "scale_x": c(5.0), "scale_y": c(22.0)}),
        &[("image", noise)],
    )?;
    let haze = scene.add(
        "Warm haze",
        &COLORIZE,
        json!({"low": [-0.13, -0.082, -0.043, 1.0], "high": [0.2, 0.125, 0.066, 1.0]}),
        &[("image", streaks)],
    )?;
    let hazy = scene.add(
        "Haze on the wall",
        &BLEND,
        json!({"mode": "add"}),
        &[("background", wall), ("foreground", haze)],
    )?;
    let glow_shape = scene.add(
        "Beam shape",
        &ELLIPSE,
        json!({"x": c(cx), "y": c(cy), "radius_x": c(600.0 * s), "radius_y": c(45.0 * s), "feather": c(300.0 * s)}),
        &[],
    )?;
    let glow_turned = scene.add(
        "Beam angle",
        &AFFINE,
        json!({"pivot_x": c(cx), "pivot_y": c(cy), "rotation": c(-28.0)}),
        &[("image", glow_shape)],
    )?;
    let black = scene.add("Black", &SOLID, json!({"color": [0.0, 0.0, 0.0, 1.0]}), &[])?;
    let glow_flat = scene.add(
        "Beam on black",
        &MERGE,
        json!({}),
        &[("background", black), ("foreground", glow_turned)],
    )?;
    let glow = scene.add(
        "Warm beam",
        &COLORIZE,
        json!({"low": [0.0, 0.0, 0.0, 1.0], "high": [0.11, 0.07, 0.037, 1.0]}),
        &[("image", glow_flat)],
    )?;
    let lit = scene.add(
        "Beam on the wall",
        &BLEND,
        json!({"mode": "add"}),
        &[("background", hazy), ("foreground", glow)],
    )?;
    let ramp = scene.add(
        "Vignette ramp",
        &GRADIENT,
        json!({"mode": "radial", "start_x": c(cx), "start_y": c(cy - 20.0 * s), "end_x": c(cx + 1350.0 * s), "end_y": c(cy - 20.0 * s)}),
        &[],
    )?;
    let vignette = scene.add(
        "Vignette",
        &COLORIZE,
        json!({"low": [1.0, 1.0, 1.0, 1.0], "high": [0.5, 0.5, 0.52, 1.0]}),
        &[("image", ramp)],
    )?;
    let backdrop = scene.add(
        "Backdrop",
        &BLEND,
        json!({"mode": "multiply"}),
        &[("background", lit), ("foreground", vignette)],
    )?;
    let mut picture = scene.add(
        "Title on the wall",
        &MERGE,
        json!({}),
        &[("background", backdrop), ("foreground", render)],
    )?;
    if !subtitle.is_empty() {
        let line = scene.add(
            "Subtitle",
            &TEXT,
            json!({
                "content": subtitle, "font": {"source": "sans"}, "size": c(36.0 * s),
                "x": c(cx), "y": c(cy + 310.0 * s), "tracking": c(14.0 * s),
                "color": [0.62, 0.6, 0.56, 1.0],
            }),
            &[],
        )?;
        picture = scene.add(
            "Subtitle on the title",
            &MERGE,
            json!({}),
            &[("background", picture), ("foreground", line)],
        )?;
    }
    // The hottest light spills onto the wall, with a short lens streak.
    let output = scene.add(
        "Bloom",
        &BLOOM,
        json!({
            "threshold": c(1.6), "knee": c(0.5), "intensity": c(0.4),
            "radius": c((70.0 * s).clamp(1.0, 256.0)), "streak": c(0.35),
            "tint": [1.0, 0.86, 0.66, 1.0],
        }),
        &[("image", picture)],
    )?;
    Ok(scene.built(
        output,
        Some(title),
        vec![
            Command::Display {
                display: DisplayTransform::Filmic,
            },
            Command::Output { id: output },
        ],
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Document, Operation, Time, blank};
    #[test]
    fn the_scene_applies_at_any_size_as_one_valid_transaction() {
        for (w, h) in [(1920, 1080), (640, 360), (3840, 2160)] {
            let mut doc = Document::new(blank(w, h, Time::new(24, 1).unwrap()).unwrap()).unwrap();
            let built = gold_title(doc.snapshot(), "GOLD", "A LINE BELOW").unwrap();
            doc.apply(0, &built.commands).unwrap();
            let after = doc.snapshot();
            assert_eq!(after.output, built.output);
            assert_eq!(after.display, DisplayTransform::Filmic);
            let rows = after
                .nodes
                .values()
                .filter(|n| matches!(&n.operation, Operation::Light3d(l) if l.strips.at(Time::new(0, 1).unwrap()).unwrap() > 1.0))
                .count();
            assert_eq!(rows, 3);
            // One undo removes all of it.
            doc.undo(doc.revision()).unwrap();
            assert_eq!(doc.snapshot().nodes.len(), 1);
        }
        // Long titles are made smaller so they fit the frame; short ones keep the size.
        let size = |text: &str| {
            let doc = blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
            let built = gold_title(&doc, text, "").unwrap();
            built
                .commands
                .iter()
                .find_map(|c| match c {
                    Command::Put { node } if Some(node.id) == built.edit => match &node.operation {
                        Operation::Text(t) => Some(t.size.at(Time::new(0, 1).unwrap()).unwrap()),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(size("VEYLVIK"), 340.0);
        assert!(size("YOUR TITLE") < 270.0 && size("YOUR TITLE") * 0.6 * 10.0 < 1920.0 * 0.85);
        let doc = blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
        let plain = gold_title(&doc, "GOLD", "").unwrap();
        let with = gold_title(&doc, "GOLD", "SUB").unwrap();
        assert_eq!(with.commands.len(), plain.commands.len() + 2);
    }
}
