//! Editable mural example. Camera motion is authored, not claimed as an analyzed track.
use super::*;
use crate::planar::{Sample, Track, WarpMode};
pub(super) fn build() -> Result<Scene> {
    let mut doc = crate::blank(960, 540, Time::new(24, 1)?)?;
    doc.frame_count = 96;
    let white = doc.output;
    doc.nodes.get_mut(&white).unwrap().operation =
        Operation::Solid(crate::nodes::Solid { color: [1.; 4] });
    let plate = add(
        &mut doc,
        "Brick wall",
        Operation::Material(crate::nodes::Material {
            smooth_edges: true,
            brushing: Default::default(),
            colours: None,
            optics: None,
            studio: Default::default(),
            kind: crate::MaterialKind::BrickRed,
            scale: 150.0.into(),
            bevel: 3.0.into(),
            roughness: 0.8.into(),
            lighting: 0.0.into(),
            grain: 0.8.into(),
            emission: 1.0.into(),
            shot: 1.0.into(),
        }),
        &[("image", white)],
    );
    doc.nodes.get_mut(&plate).unwrap().scene_background = true;
    let reference = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let closer = [[-0.06, -0.05], [1.08, -0.01], [1.03, 1.08], [-0.04, 1.03]];
    let track = add(
        &mut doc,
        "Authored camera motion (example)",
        Operation::PlanarTrack(crate::nodes::PlanarTrack {
            data: Box::new(Track {
                reference_time: Time::new(0, 1)?,
                reference,
                region: None,
                samples: [(0, reference), (2, closer), (4, reference)]
                    .into_iter()
                    .map(|(ticks, corners)| Sample {
                        time: Time { ticks, rate: 1 },
                        corners,
                        features: vec![],
                        rms_pixels: 0.,
                        motion: None,
                    })
                    .collect(),
            }),
        }),
        &[("image", plate)],
    );
    let moving_plate = add(
        &mut doc,
        "Wall follows camera",
        Operation::PlanarWarp(crate::nodes::PlanarWarp {
            mode: WarpMode::Follow,
        }),
        &[("image", plate), ("track", track)],
    );
    let mut text = title_text("STAY\nCURIOUS");
    if let Operation::Text(crate::nodes::Text {
        size,
        y,
        tracking,
        color,
        ..
    }) = &mut text
    {
        *size = 105.0.into();
        *y = 235.0.into();
        *tracking = 5.0.into();
        *color = [0.95, 0.8, 0.45, 1.];
    }
    let title = add(&mut doc, "Editable mural lettering", text, &[]);
    let moving_title = add(
        &mut doc,
        "Lettering follows same camera",
        Operation::PlanarWarp(crate::nodes::PlanarWarp {
            mode: WarpMode::Follow,
        }),
        &[("image", title), ("track", track)],
    );
    let before = add(
        &mut doc,
        "Before: clean lettering on wall",
        Operation::Merge(crate::nodes::Merge {}),
        &[("background", moving_plate), ("foreground", moving_title)],
    );
    doc.output = before;
    let preset = super::super::builtin::build("surface_paint")?;
    let bindings = preset
        .group
        .inputs
        .iter()
        .map(|(key, input)| {
            let source = if input.targets[0].port == "background" {
                moving_plate
            } else {
                moving_title
            };
            (key.clone(), source)
        })
        .collect();
    let instance = preset.instantiate(&doc, &bindings, None)?;
    let mut composition = Document::new(doc)?.prepare(0, &instance.commands)?;
    composition.output = instance.output;
    super::super::resolution::map_graph(
        &mut composition.nodes,
        &mut composition.groups,
        [960, 540],
        [1920, 1080],
        super::super::resolution::Sizing::Fit,
    )?;
    composition.width = 1920;
    composition.height = 1080;
    composition.validate()?;
    Ok(Scene {
        composition,
        before,
    })
}
