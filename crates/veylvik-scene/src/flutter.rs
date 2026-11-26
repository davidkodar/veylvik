//! Flutter: travelling waves bent into a mesh out of its plane (its own z), growing from a pinned left edge if asked, with a smaller ripple across them; normals are worked out again from the bent triangles, so lighting follows the folds.
use veylvik_mesh::Mesh;

/// The waves a Flutter node puts through the objects beneath it, at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flutter {
    /// How far the surface swings out of its plane (pixels).
    pub amplitude: f32,
    /// Crest to crest (pixels).
    pub wavelength: f32,
    /// How fast the waves travel along the shape (pixels a second).
    pub speed: f32,
    /// 1 holds the left edge still, 0 moves all of it alike.
    pub pinned: f32,
    /// The moment, in seconds.
    pub time: f32,
}
/// Bends `mesh` by `waves`. `across` is the span of x the waves are measured over (the
/// left edge is its start): the mesh's own when not given, a whole model's for its parts.
pub fn flutter(mesh: &mut Mesh, waves: &Flutter, across: Option<[f32; 2]>) {
    if waves.amplitude <= 0.0 || mesh.vertices.is_empty() {
        return;
    }
    let [left, right] = across.unwrap_or_else(|| {
        mesh.vertices
            .iter()
            .fold([f32::MAX, f32::MIN], |[a, b], v| {
                [a.min(v.position[0]), b.max(v.position[0])]
            })
    });
    let span = (right - left).max(1e-3);
    let k = std::f32::consts::TAU / waves.wavelength.max(1.0);
    let travel = k * waves.speed * waves.time;
    for v in &mut mesh.vertices {
        let [x, y, _] = v.position;
        let along = ((x - left) / span).clamp(0.0, 1.0);
        // From the pole the swing grows to the free end; unpinned it is the same all over.
        let reach = 1.0 - waves.pinned.clamp(0.0, 1.0) * (1.0 - along.powf(0.85));
        let main = (k * (x - left) - travel + 0.35 * k * y).sin();
        let ripple = 0.3 * (2.3 * k * (x - left) - 1.7 * travel + 0.9 * k * y + 1.1).sin();
        v.position[2] += waves.amplitude * reach * (main + ripple);
    }
    // Normals from the bent triangles, each weighted by its area.
    let mut normals = vec![[0.0f32; 3]; mesh.vertices.len()];
    for t in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[t[i] as usize].position);
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        for &i in t {
            // A two-sided shape shares its corners between front and back: each face
            // counts on the side its corner faced, or the two would cancel.
            let was = mesh.vertices[i as usize].normal;
            let side = if n[0] * was[0] + n[1] * was[1] + n[2] * was[2] >= 0.0 {
                1.0
            } else {
                -1.0
            };
            for k in 0..3 {
                normals[i as usize][k] += n[k] * side;
            }
        }
    }
    for (v, n) in mesh.vertices.iter_mut().zip(normals) {
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length > 1e-12 {
            // Keep the side the shape faced before, so a surface does not turn inside out.
            let n = n.map(|c| c / length);
            let same = n[0] * v.normal[0] + n[1] * v.normal[1] + n[2] * v.normal[2] >= 0.0;
            v.normal = if same { n } else { n.map(|c| -c) };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_pinned_banner_stays_still_at_its_pole_and_swings_at_its_free_end() {
        let mut banner = veylvik_mesh::bendable_plane([400.0, 200.0], 64);
        let flat = banner.clone();
        let waves = Flutter {
            amplitude: 30.0,
            wavelength: 200.0,
            speed: 100.0,
            pinned: 1.0,
            time: 0.7,
        };
        flutter(&mut banner, &waves, None);
        let swing = |at: f32| {
            banner
                .vertices
                .iter()
                .filter(|v| (v.position[0] - at).abs() < 4.0)
                .map(|v| v.position[2].abs())
                .fold(0.0f32, f32::max)
        };
        assert!(swing(-200.0) < 0.5, "the pole edge moved {}", swing(-200.0));
        assert!(
            swing(200.0) > 15.0,
            "the free end swings only {}",
            swing(200.0)
        );
        // Only depth changes, and the normals tip with the folds.
        assert!(
            banner
                .vertices
                .iter()
                .zip(&flat.vertices)
                .all(|(a, b)| a.position[0] == b.position[0] && a.position[1] == b.position[1])
        );
        assert!(banner.vertices.iter().any(|v| v.normal[0].abs() > 0.2));
        // Unpinned, the whole banner moves; at no amplitude nothing does.
        let mut loose = flat.clone();
        flutter(
            &mut loose,
            &Flutter {
                pinned: 0.0,
                ..waves
            },
            None,
        );
        assert!(
            loose
                .vertices
                .iter()
                .filter(|v| v.position[0] < -190.0)
                .any(|v| v.position[2].abs() > 5.0)
        );
        let mut still = flat.clone();
        flutter(
            &mut still,
            &Flutter {
                amplitude: 0.0,
                ..waves
            },
            None,
        );
        assert_eq!(
            still
                .vertices
                .iter()
                .map(|v| v.position)
                .collect::<Vec<_>>(),
            flat.vertices.iter().map(|v| v.position).collect::<Vec<_>>()
        );
    }
}
