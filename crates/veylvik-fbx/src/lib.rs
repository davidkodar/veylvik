//! FBX scenes read into Veylvik models, the same as an imported glTF or USD scene: every mesh with its material, the file's animation baked frame by frame (parts that move as one keyed as they move, parts that bend as point caches), and its cameras keyed in the model's frame. The reading is done by ufbx (a C library) in a helper process (`veylvik fbx-read`), which sends the model back in a compact binary form, so a damaged or hostile file can only take the helper down.
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use veylvik_mesh::Model;

mod read;
mod wire;
pub use read::read;
pub use wire::{decode, encode};

/// The `veylvik` subcommand that reads one FBX file and answers with the model.
pub const SUBCOMMAND: &str = "fbx-read";
/// The file extensions read as FBX.
pub const EXTENSIONS: [&str; 1] = ["fbx"];
/// The most bytes a helper may answer with.
const MOST_BYTES: usize = 1 << 31;
/// How long a helper may take over one file.
const PATIENCE: Duration = Duration::from_secs(300);

/// The FBX file at `path` as a model, read by `program fbx-read` (the `veylvik` program)
/// in a process of its own.
pub fn load(program: &Path, path: &Path) -> Result<Model, String> {
    let mut child = Command::new(program)
        .arg(SUBCOMMAND)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the FBX reader could not start: {e}"))?;
    let mut out = child.stdout.take().ok_or("the FBX reader has no output")?;
    let mut err = child.stderr.take().ok_or("the FBX reader has no output")?;
    // Read both streams on threads, so a large answer never blocks the helper.
    let reading = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = (&mut out)
            .take(MOST_BYTES as u64 + 1)
            .read_to_end(&mut bytes);
        read.map(|_| bytes)
    });
    let complaining = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = (&mut err).take(64 * 1024).read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the FBX reader took too long over this file and was stopped".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let bytes = reading
        .join()
        .map_err(|_| "the FBX reader's output was lost")?
        .map_err(|e| e.to_string())?;
    let complaint = complaining.join().unwrap_or_default();
    if !status.success() {
        let said = complaint.trim();
        return Err(if said.is_empty() {
            format!("the FBX reader stopped on this file ({status}): the file may be damaged")
        } else {
            said.to_owned()
        });
    }
    if bytes.len() > MOST_BYTES {
        return Err("the FBX reader's answer is too large".into());
    }
    decode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scene.fbx")
    }
    #[test]
    fn a_blender_scene_reads_with_its_parts_animation_and_camera() {
        let model = read(&fixture()).unwrap();
        // The card and the moving cube.
        assert_eq!(
            model.parts.len(),
            2,
            "{:?}",
            model.parts.iter().map(|p| p.colour).collect::<Vec<_>>()
        );
        assert!(
            model
                .parts
                .iter()
                .any(|p| p.colour[0] > 0.9 && p.colour[1] < 0.1),
            "the red card"
        );
        assert_eq!(model.clips.len(), 1);
        assert!(
            (model.clips[0].seconds() - 1.0).abs() < 0.01,
            "{}",
            model.clips[0].seconds()
        );
        assert!(
            model.parts.iter().filter(|p| p.joint.is_some()).count() == 1,
            "the cube moves"
        );
        assert_eq!(model.cameras.len(), 1);
        let camera = &model.cameras[0];
        assert!(camera.keys.len() > 2 && (camera.keys[0].focal_length - 50.0).abs() < 0.01);
        assert!((camera.keys[0].sensor_width - 36.0).abs() < 0.01);
        // It glides 1 along x while the cube travels 4 (in whatever unit the file has).
        let (first, last) = (camera.keys[0], *camera.keys.last().unwrap());
        let moved = (last.position[0] - first.position[0]) * model.largest;
        let travel = &model.clips[0].channels[0].values;
        let cube = travel.last().unwrap()[0] - travel[0][0];
        assert!((moved / cube - 0.25).abs() < 0.01, "{moved} {cube}");
    }
    #[test]
    fn a_model_survives_the_trip_from_the_helper_unchanged() {
        let model = read(&fixture()).unwrap();
        let bytes = encode(&model);
        assert_eq!(encode(&decode(&bytes).unwrap()), bytes);
        // Cut short, or with something left over, it is refused rather than misread.
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode(&longer).is_err());
        assert!(decode(b"not a model").is_err());
    }
}
