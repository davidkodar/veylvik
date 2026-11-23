use super::*;
use veylvik_document::Scalar;

const SCRIPT: &str = r#"#! /usr/local/Nuke15.0v4/libnuke-15.0.4.so -nx
version 15.0 v4
Root {
 inputs 0
 name /shots/a/comp.nk
 first_frame 1001
 last_frame 1100
 fps 23.976
 format "2048 1152 0 0 2048 1152 1 2K_16x9"
}
Constant {
 inputs 0
 color {0.1 0.2 0.3 1}
 name Backdrop
 xpos -200
 ypos -100
}
set N1 [stack 0]
Read {
 inputs 0
 file /plates/a/plate.%04d.exr
 name Plate
 xpos 0
 ypos -300
}
Grade {
 gain {{curve x1001 1 x1050 2}}
 gamma 1.2
 name Grade1
}
Transform {
 translate {100 50}
 rotate 30
 center {1024 576}
 name Move
}
Blur {
 size 8
 name Soft
}
Dot {
 name Dot1
}
OFlow2 {
 name Retime
}
Group {
 name Tool
}
 Input {
  inputs 0
  name Input1
 }
 Output {
  name Output1
 }
end_group
push $N1
Merge2 {
 inputs 2
 name Over
}
Write {
 file /renders/a.%04d.exr
 name Out
}
"#;
fn operation<'a>(c: &'a Composition, label: &str) -> &'a Node {
    c.nodes
        .values()
        .find(|n| n.label == label)
        .unwrap_or_else(|| panic!("no {label}"))
}
fn constant(s: &Scalar) -> f32 {
    match s {
        Scalar::Constant { value } => *value,
        other => panic!("not constant: {other:?}"),
    }
}
#[test]
fn a_script_is_rebuilt_wired_as_it_was() {
    let imported = import(SCRIPT).unwrap();
    let c = &imported.composition;
    assert_eq!((c.width, c.height, c.frame_count), (2048, 1152, 100));
    assert_eq!((c.frame_rate.ticks, c.frame_rate.rate), (24000, 1001));
    // The Merge lays the plate's branch (A, its second input) over the Constant (B).
    let over = operation(c, "Over");
    assert!(matches!(over.operation, Operation::Merge(_)));
    assert_eq!(c.output, over.id, "the Write takes the Merge");
    let backdrop = operation(c, "Backdrop");
    assert_eq!(over.inputs["background"], backdrop.id);
    let soft = operation(c, "Soft");
    // The Dot, the unknown node and the closed group pass the blur on.
    assert_eq!(over.inputs["foreground"], soft.id);
    let Operation::Blur(blur) = &soft.operation else {
        panic!()
    };
    assert_eq!(constant(&blur.radius), 4.0);
    let moved = operation(c, "Move");
    assert_eq!(soft.inputs["image"], moved.id);
    let Operation::Affine(a) = &moved.operation else {
        panic!()
    };
    // Nuke's y is up and its turns anticlockwise.
    assert_eq!(
        (constant(&a.x), constant(&a.y), constant(&a.rotation)),
        (100.0, -50.0, -30.0)
    );
    assert_eq!(
        (constant(&a.pivot_x), constant(&a.pivot_y)),
        (1024.0, 1152.0 - 576.0)
    );
    let grade = operation(c, "Grade1");
    assert_eq!(moved.inputs["image"], grade.id);
    let Operation::ColorCorrect(g) = &grade.operation else {
        panic!()
    };
    assert_eq!(constant(&g.gamma), 1.2);
    // The keyed gain: frame 1001 is the composition's frame 0.
    match &g.gain {
        Scalar::Smooth { keys } => {
            assert_eq!(keys.len(), 2);
            assert_eq!((keys[0].value, keys[1].value), (1.0, 2.0));
            assert_eq!(keys[0].time.ticks, 0);
            let at = Time::at_frame(49, c.frame_rate).unwrap();
            assert_eq!((keys[1].time.ticks, keys[1].time.rate), (at.ticks, at.rate));
        }
        other => panic!("not keyed: {other:?}"),
    }
    let plate = operation(c, "Plate");
    assert_eq!(grade.inputs["image"], plate.id);
    assert!(plate.position.is_some(), "placed as in Nuke");
    // What did not come over is named, not lost.
    assert_eq!(
        imported.not_rebuilt,
        vec!["Retime (OFlow2)".to_string(), "Tool (Group)".to_string()]
    );
    assert!(imported.notes.iter().any(|n| n.contains("image sequence")));
    assert!(imported.notes.iter().any(|n| n.contains("group")));
    assert_eq!(imported.rebuilt, 6);
}
#[test]
fn a_merge_mode_and_mix_become_a_blend() {
    let script = "Root {\n format \"640 360 0 0 640 360 1 small\"\n}\nConstant {\n inputs 0\n color 0.5\n name B\n}\nConstant {\n inputs 0\n color 1\n name A\n}\nMerge2 {\n inputs 2\n operation screen\n mix 0.25\n name Screen\n}\n";
    let c = import(script).unwrap().composition;
    let screen = operation(&c, "Screen");
    let Operation::Blend(b) = &screen.operation else {
        panic!("{:?}", screen.operation)
    };
    assert_eq!(constant(&b.opacity), 0.25);
    assert_eq!(serde_json::to_value(b.mode).unwrap(), "screen");
    assert_eq!(c.output, screen.id, "with no Write, the last node");
}
#[test]
fn a_sequence_stands_in_with_its_first_frame() {
    assert_eq!(first_frame("/p/plate.%04d.exr", 1001), "/p/plate.1001.exr");
    assert_eq!(first_frame("/p/plate.%d.exr", 7), "/p/plate.7.exr");
    assert_eq!(first_frame("/p/plate.####.exr", 12), "/p/plate.0012.exr");
    assert_eq!(first_frame("/p/still.exr", 12), "/p/still.exr");
    let c = import(SCRIPT).unwrap().composition;
    let Operation::Footage(f) = &operation(&c, "Plate").operation else {
        panic!()
    };
    // No `first` on the Read: the script's first frame.
    assert_eq!(f.asset, "/plates/a/plate.1001.exr");
}
#[test]
fn an_expression_keeps_the_default_and_is_named() {
    let script = "Constant {\n inputs 0\n name C\n}\nGrade {\n gain {{parent.Other.gain}}\n gamma 2\n name G\n}\n";
    let imported = import(script).unwrap();
    let Operation::ColorCorrect(g) = &operation(&imported.composition, "G").operation else {
        panic!()
    };
    assert_eq!((constant(&g.gain), constant(&g.gamma)), (1.0, 2.0));
    assert!(
        imported
            .notes
            .iter()
            .any(|n| n.contains("G: gain is an expression")),
        "{:?}",
        imported.notes
    );
}
#[test]
fn what_is_not_a_script_is_refused() {
    assert!(import("Grade {\n gain {1 2\n}").is_err());
}
