//! USD scenes written as text, read into models: the up axis, transform ops in their order, keyed transforms as a clip, points keyed as a cache, the bound material, and polygons into triangles.
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn write(name: &str, text: &str) -> Scratch {
    let path = std::env::temp_dir().join(format!("veylvik-usd-{}-{name}.usda", std::process::id()));
    std::fs::write(&path, text).unwrap();
    Scratch(path)
}
fn close(a: [f32; 3], b: [f32; 3], within: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < within)
}
/// A unit square in x and y, at z = `z`, as one quad.
fn square(name: &str, extra: &str, z: f32) -> String {
    format!(
        r#"
    def Mesh "{name}"
    {{
        int[] faceVertexCounts = [4]
        int[] faceVertexIndices = [0, 1, 2, 3]
        point3f[] points = [(0, 0, {z}), (1, 0, {z}), (1, 1, {z}), (0, 1, {z})]
        {extra}
    }}"#
    )
}
#[test]
fn a_z_up_quad_stands_up_the_right_way_round_in_two_triangles() {
    let file = write(
        "zup",
        &format!(
            "#usda 1.0\n(\n    upAxis = \"Z\"\n)\ndef Xform \"World\"\n{{{}\n}}\n",
            square("Floor", "", 0.0)
        ),
    );
    let model = veylvik_usd::load(&file.0).unwrap();
    assert_eq!(model.parts.len(), 1);
    let mesh = &model.parts[0].mesh;
    assert_eq!(mesh.indices.len(), 6, "a quad is two triangles");
    // A square lying in x and y with z up is a floor once y is up: flat in y.
    assert!(model.extent[1] < 1e-6, "{:?}", model.extent);
    assert!(model.extent[0] > 0.99 && model.extent[2] > 0.99);
    // It faces up (+z in the file, -y in Veylvik's frame, y down).
    for v in &mesh.vertices {
        assert!(close(v.normal, [0.0, -1.0, 0.0], 1e-5), "{:?}", v.normal);
    }
    // Triangles turn the way their normals face, as an imported glTF's do.
    for t in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| mesh.vertices[i as usize].position);
        let (u, v) = (
            [0, 1, 2].map(|k| b[k] - a[k]),
            [0, 1, 2].map(|k| c[k] - a[k]),
        );
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let normal = mesh.vertices[t[0] as usize].normal;
        assert!(
            n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2] > 0.0,
            "{n:?}"
        );
    }
}
#[test]
fn transform_ops_apply_in_their_order_under_their_parents() {
    // The parent doubles everything; the child is turned a quarter about z, then moved
    // 5 along x (translate is listed first: outermost).
    let file = write(
        "ops",
        &format!(
            r#"#usda 1.0
def Xform "Parent"
{{
    float3 xformOp:scale = (2, 2, 2)
    uniform token[] xformOpOrder = ["xformOp:scale"]
    def Xform "Child"
    {{
        double3 xformOp:translate = (5, 0, 0)
        float xformOp:rotateZ = 90
        uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:rotateZ"]
        {}
    }}
}}
"#,
            square("Card", "", 0.0)
        ),
    );
    let model = veylvik_usd::load(&file.0).unwrap();
    // Corner (1, 0) turns to (0, 1), moves to (5, 1), doubles to (10, 2); corner (0, 1)
    // turns to (-1, 0): x from 8 to 10, y from 0 to 2. Normalised by 2 and centred.
    let largest = model.largest;
    assert!((largest - 2.0).abs() < 1e-5, "{largest}");
    // The centre is kept in Veylvik's frame, y down.
    let centre = [9.0, -1.0, 0.0];
    for (k, c) in centre.iter().enumerate() {
        let got = model.centre[k];
        assert!((got - c).abs() < 1e-5, "centre {:?}", model.centre);
    }
}
#[test]
fn a_mesh_moved_by_its_own_transform_moves_once() {
    let file = write(
        "own",
        r#"#usda 1.0
def Mesh "Card"
{
    int[] faceVertexCounts = [4]
    int[] faceVertexIndices = [0, 1, 2, 3]
    point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]
    double3 xformOp:translate = (5, 0, 0)
    uniform token[] xformOpOrder = ["xformOp:translate"]
}
"#,
    );
    let model = veylvik_usd::load(&file.0).unwrap();
    // Its box runs from x 5 to 6: its middle at 5.5 (y down: -0.5).
    assert!(
        close(model.centre, [5.5, -0.5, 0.0], 1e-5),
        "{:?}",
        model.centre
    );
}
#[test]
fn keyed_transforms_become_the_models_clip() {
    let file = write(
        "keys",
        &format!(
            r#"#usda 1.0
(
    startTimeCode = 1
    endTimeCode = 25
    timeCodesPerSecond = 24
)
def Xform "Mover"
{{
    double3 xformOp:translate.timeSamples = {{ 1: (0, 0, 0), 25: (10, 0, 0) }}
    uniform token[] xformOpOrder = ["xformOp:translate"]
    {}
}}
"#,
            square("Card", "", 0.0)
        ),
    );
    let model = veylvik_usd::load(&file.0).unwrap();
    assert_eq!(model.clips.len(), 1);
    assert!((model.clips[0].seconds() - 1.0).abs() < 1e-5);
    let start = model.poses(0, 0.0, false).unwrap()[0];
    let end = model.poses(0, 1.0, false).unwrap()[0];
    let half = model.poses(0, 0.5, false).unwrap()[0];
    // At rest it has not moved; a second in it is 10 along x (in the normalised frame,
    // over its largest side, 1).
    assert!((start[0][3]).abs() < 1e-5);
    assert!((end[0][3] - 10.0).abs() < 1e-4, "{end:?}");
    assert!((half[0][3] - 5.0).abs() < 1e-4, "{half:?}");
}
#[test]
fn keyed_points_become_a_cache_that_bends_the_part() {
    let file = write(
        "cache",
        r#"#usda 1.0
(
    startTimeCode = 0
    endTimeCode = 10
    timeCodesPerSecond = 10
)
def Mesh "Sheet"
{
    int[] faceVertexCounts = [4]
    int[] faceVertexIndices = [0, 1, 2, 3]
    point3f[] points.timeSamples = {
        0: [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)],
        10: [(0, 0, 0), (1, 0, 0), (1, 1, 1), (0, 1, 1)],
    }
}
"#,
    );
    let model = veylvik_usd::load(&file.0).unwrap();
    let part = &model.parts[0];
    assert!(part.cache.is_some() && part.deforms());
    assert_eq!(model.clips.len(), 1);
    assert!((model.clips[0].seconds() - 1.0).abs() < 1e-5);
    let first = model.deformed(0, Some((0, 0.0, false))).unwrap();
    let last = model.deformed(0, Some((0, 1.0, false))).unwrap();
    let half = model.deformed(0, Some((0, 0.5, false))).unwrap();
    // The third corner (index 2) rises from z 0 to z 1: halfway it is at a half.
    let z = |m: &veylvik_mesh::Mesh| m.vertices[2].position[2] * model.largest + model.centre[2];
    assert!(z(&first).abs() < 1e-5, "{}", z(&first));
    assert!((z(&last) - 1.0).abs() < 1e-5, "{}", z(&last));
    assert!((z(&half) - 0.5).abs() < 1e-5, "{}", z(&half));
    // The first corner never moves.
    assert!(close(
        first.vertices[0].position,
        last.vertices[0].position,
        1e-6
    ));
}
#[test]
fn the_bound_preview_surface_gives_colour_metal_and_roughness() {
    let file = write(
        "material",
        &format!(
            r#"#usda 1.0
def Xform "World"
{{
    def Material "Gold"
    {{
        token outputs:surface.connect = </World/Gold/Surface.outputs:surface>
        def Shader "Surface"
        {{
            uniform token info:id = "UsdPreviewSurface"
            color3f inputs:diffuseColor = (1, 0.5, 0.1)
            float inputs:metallic = 1
            float inputs:roughness = 0.2
            token outputs:surface
        }}
    }}
    {}
}}
"#,
            square(
                "Plate",
                "rel material:binding = </World/Gold>\n        color3f[] primvars:displayColor = [(0, 0, 1)]",
                0.0
            )
        ),
    );
    let model = veylvik_usd::load(&file.0).unwrap();
    let part = &model.parts[0];
    assert_eq!(part.colour, [1.0, 0.5, 0.1, 1.0]);
    assert_eq!((part.metallic, part.roughness), (1.0, 0.2));
}
#[test]
fn a_file_without_meshes_says_so() {
    let file = write("empty", "#usda 1.0\ndef Xform \"Nothing\"\n{\n}\n");
    let error = veylvik_usd::load(&file.0).unwrap_err();
    assert!(error.contains("no meshes"), "{error}");
}
