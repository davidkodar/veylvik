//! Model catalogue and installer. Every byte is pinned by revision URL, size and SHA-256, is
//! downloaded only on explicit request, and lands in `<models>/<id>/` only after verification.
use crate::Result;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// One verified file of a model. `range` fetches an inclusive byte span of a larger file.
pub struct File {
    pub name: &'static str,
    pub url: &'static str,
    pub range: Option<(u64, u64)>,
    pub size: u64,
    pub sha256: &'static str,
}
pub struct Model {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub licence: &'static str,
    pub homepage: &'static str,
    /// What the model is used for in Veylvik, for the model's information panel.
    pub uses: &'static str,
    pub pros: &'static [&'static str],
    pub cons: &'static [&'static str],
    /// Typical speed and graphics memory, as measured or published.
    pub speed: &'static str,
    pub memory: &'static str,
    pub files: &'static [File],
}
impl Model {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Two small tensors the exported graphs leave to the host, read from the upstream checkpoint.
const EDGETAM_CHECKPOINT: &str = "https://huggingface.co/yonigozlan/EdgeTAM-hf/resolve/c266ce53b3fc00f0f495b583f6a116c4e57f53bb/model.safetensors";
macro_rules! graph {
    ($name:literal, $size:expr, $sha:literal) => {
        File {
            name: $name,
            url: concat!(
                "https://huggingface.co/jax-image-tools/edgetam-video-onnx/resolve/8ca3d3e4169938e65b552cdf14542fde25e8badb/",
                $name
            ),
            range: None,
            size: $size,
            sha256: $sha,
        }
    };
}
pub static EDGETAM: Model = Model {
    id: "edgetam",
    title: "EdgeTAM",
    summary: "Fast click-to-mask and video tracking (SAM 2 with an efficient memory), about 30 ms a frame on a desktop GPU.",
    licence: "Apache-2.0",
    homepage: "https://huggingface.co/facebook/EdgeTAM",
    uses: "Subject masks: click a person or object in the viewer, then track it through the shot.",
    pros: &[
        "Fast: about 30 ms a frame on an RTX 4080, so previews feel instant",
        "Tracks through video with SAM 2's memory, including brief occlusions",
        "Small download and light on graphics memory",
        "Apache-2.0 licence, runs entirely on this computer",
    ],
    cons: &[
        "Slightly less precise than SAM 2.1 on difficult, small or thin subjects",
        "Raw edges are soft and coarse at 4K; Veylvik's edge refinement improves them",
        "Needs an NVIDIA GPU with CUDA and cuDNN",
    ],
    speed: "≈30 ms per frame (RTX 4080)",
    memory: "≈1.5 GB (measured on an RTX 4080)",
    files: &[
        graph!(
            "vision_encoder.onnx",
            19_826_101,
            "57fe1a2b3d500813fe4987c4209b856920f187ab0b7723d81586b607ea2cffc1"
        ),
        graph!(
            "mask_decoder.onnx",
            17_800_677,
            "ee24ee1cae6ecc71889912c1893e08b09f645bb63875108ccd180f7218a3647d"
        ),
        graph!(
            "memory_attention.onnx",
            20_914_958,
            "6477d775639905945949508c08a2c652d01e5933c0eb00a02225ae5f6aa1602c"
        ),
        graph!(
            "memory_encoder.onnx",
            6_692_568,
            "035b30f8fb1f7c99211ada6eb9d72f0819de819ef94310b14e8147dc97f120ec"
        ),
        File {
            name: "no_memory_embedding.f32",
            url: EDGETAM_CHECKPOINT,
            range: Some((34_385_992, 34_387_015)),
            size: 1024,
            sha256: "f974bd1e6436998ba8c372cdcbe768c5ab39ee942d847494c709977db0592e9d",
        },
        File {
            name: "memory_temporal_positional_encoding.f32",
            url: EDGETAM_CHECKPOINT,
            range: Some((34_384_200, 34_385_991)),
            size: 1792,
            sha256: "f736c5694db176c96da7dbafe6b5b36c5375dc52bd4c358c328fa807887f80f9",
        },
    ],
};
pub static VITMATTE: Model = Model {
    id: "vitmatte",
    title: "ViTMatte",
    summary: "Image matting for hair and soft edges: refines the band around a tracked subject's outline from the image itself.",
    licence: "Apache-2.0",
    homepage: "https://huggingface.co/hustvl/vitmatte-small-composition-1k",
    uses: "Hair and soft edges for subject masks: strands, fur and motion blur along the outline.",
    pros: &[
        "Recovers fine hair and semi-transparent edges that segmentation models cannot",
        "Works on any subject, guided by the tracked outline",
        "Only the edge band is processed, so the rest of the matte stays exactly as tracked",
        "Apache-2.0 weights, runs entirely on this computer",
    ],
    cons: &[
        "Trained on academic matting datasets (Composition-1k) whose terms are research-oriented; check them before commercial use",
        "Adds time to every analysed frame",
        "Needs an NVIDIA GPU with CUDA and cuDNN",
    ],
    speed: "≈55 ms per frame for a full-length person at 1080 × 1920 (RTX 4080)",
    memory: "≈0.5 GB more for a full-length person at 1080 × 1920; more for large subjects",
    files: &[File {
        name: "model.onnx",
        url: "https://huggingface.co/Xenova/vitmatte-small-composition-1k/resolve/6bc1297f6140f055a227b6d2cfe8c093281f35d2/onnx/model.onnx",
        range: None,
        size: 103_885_865,
        sha256: "bf28d2e0be2c073286e88d60ad649d7123da2749a2d99133fd1098d5887e0225",
    }],
};
/// The larger matting model, run at the picture's full size in tiles: curls and single
/// strands where the small one gives a soft rim.
pub static VITMATTE_FINE: Model = Model {
    id: "vitmatte-fine",
    title: "ViTMatte Fine",
    summary: "The larger matting model, run at the picture's full size: resolves curls and single strands of hair where the standard one gives a soft rim.",
    licence: "Apache-2.0",
    homepage: "https://huggingface.co/hustvl/vitmatte-base-distinctions-646",
    uses: "Fine edges for subject masks: curly and flyaway hair, fur, lace, against busy or dark backgrounds.",
    pros: &[
        "Resolves individual curls and strands that the standard matting model blurs together",
        "Works at the picture's own resolution, in overlapping tiles, so 4K subjects keep their detail",
        "Only tiles that hold part of the edge band are processed",
        "Apache-2.0 weights, runs entirely on this computer",
    ],
    cons: &[
        "About three times the matting time of the standard model",
        "Trained on an academic matting dataset (Distinctions-646) whose terms are research-oriented; check them before commercial use",
        "Needs an NVIDIA GPU with CUDA and cuDNN",
    ],
    speed: "≈1 s per frame for a head and shoulders at 1080 × 1920 (RTX 4080), six tiles",
    memory: "≈2 GB more, whatever the picture's size",
    files: &[File {
        name: "model.onnx",
        url: "https://huggingface.co/Xenova/vitmatte-base-distinctions-646/resolve/80e3f7360e710e7719979206ad00a49cf1fe5ce8/onnx/model.onnx",
        range: None,
        size: 387_371_620,
        sha256: "161f71cd6ac6b336a67e8b83bee0c3b9c039292b16b3db867035a65dbcb3df32",
    }],
};
macro_rules! sam21_file {
    ($repo:literal, $rev:literal, $path:literal, $name:literal, $size:expr, $sha:literal) => {
        File {
            name: $name,
            url: concat!(
                "https://huggingface.co/",
                $repo,
                "/resolve/",
                $rev,
                "/",
                $path
            ),
            range: None,
            size: $size,
            sha256: $sha,
        }
    };
}
const SAM21_PROS: &[&str] = &[
    "SAM 2.1 quality: the most precise click-to-mask and tracking here, best on difficult, small or thin subjects",
    "The same memory-based tracking through occlusions as EdgeTAM",
    "Apache-2.0 licence, runs entirely on this computer",
];
pub static SAM21_BASE: Model = Model {
    id: "sam2.1-base-plus",
    title: "SAM 2.1 Base+",
    summary: "High-quality click-to-mask and video tracking (Meta's SAM 2.1, Hiera Base+), in half precision.",
    licence: "Apache-2.0",
    homepage: "https://huggingface.co/diffusionstudio/sam2.1-base-plus-video-onnx-fp16",
    uses: "Subject masks where EdgeTAM is not precise enough: busy scenes, small or thin subjects.",
    pros: SAM21_PROS,
    cons: &[
        "Slower than EdgeTAM; previews and tracking take longer",
        "Larger download and more graphics memory",
        "Needs an NVIDIA GPU with CUDA and cuDNN",
    ],
    speed: "≈65 ms per frame (RTX 4080), about twice EdgeTAM",
    memory: "≈2.9 GB (measured on an RTX 4080)",
    files: &[
        sam21_file!(
            "diffusionstudio/sam2.1-base-plus-video-onnx-fp16",
            "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d",
            "onnx/vision_encoder.onnx",
            "vision_encoder.onnx",
            155_490_079,
            "3cdc1e01c96f31e356b63a569676087deb1e85b08ef79d2edb8aec1fe9be19fc"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-base-plus-video-onnx-fp16",
            "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d",
            "onnx/mask_decoder.onnx",
            "mask_decoder.onnx",
            8_910_345,
            "874459f8436730bc8f9c8808263a6d8c8065437da6858726a10c373b873b8c6f"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-base-plus-video-onnx-fp16",
            "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d",
            "onnx/memory_attention.onnx",
            "memory_attention.onnx",
            16_169_156,
            "457232835ef18b7c658a3c8af9dffb4578ac4af433249840886f3ff399c15742"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-base-plus-video-onnx-fp16",
            "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d",
            "onnx/memory_encoder.onnx",
            "memory_encoder.onnx",
            2_807_648,
            "9fad9ee8c08629033bc72a8ef0cce91fb5389a4a6c18a8ea733e685cc57c839f"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-base-plus-video-onnx-fp16",
            "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d",
            "onnx/pointer_tpos.onnx",
            "pointer_tpos.onnx",
            34_228,
            "0f308871196f4312a4aec65ce62f99cb341cd4e7516fbf7bc13bc8d4d111bc14"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-base-plus-video-onnx-fp16",
            "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d",
            "constants.json",
            "constants.json",
            9_857,
            "56f2a2e17741a630099113314e8a0b722fc04e48956320c02401c603acf0d3fb"
        ),
    ],
};
pub static SAM21_LARGE: Model = Model {
    id: "sam2.1-large",
    title: "SAM 2.1 Large",
    summary: "The highest-quality click-to-mask and video tracking here (Meta's SAM 2.1, Hiera Large), in half precision.",
    licence: "Apache-2.0",
    homepage: "https://huggingface.co/diffusionstudio/sam2.1-large-video-onnx-fp16",
    uses: "The hardest subject masks, when time matters less than precision.",
    pros: SAM21_PROS,
    cons: &[
        "The slowest tracker here",
        "Largest download and the most graphics memory",
        "Needs an NVIDIA GPU with CUDA and cuDNN",
    ],
    speed: "≈80 ms per frame (RTX 4080)",
    memory: "≈3.2 GB (measured on an RTX 4080)",
    files: &[
        sam21_file!(
            "diffusionstudio/sam2.1-large-video-onnx-fp16",
            "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1",
            "onnx/vision_encoder.onnx",
            "vision_encoder.onnx",
            447_430_045,
            "7e085aa57b605b73a264be74d8ca4772c30b07af914e3f146b05ad69c6f7aafe"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-large-video-onnx-fp16",
            "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1",
            "onnx/mask_decoder.onnx",
            "mask_decoder.onnx",
            8_910_364,
            "8e1d95f38f1f7e6227a0e9bf0fa32effd592bd93c039203631d0c75e5112d97d"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-large-video-onnx-fp16",
            "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1",
            "onnx/memory_attention.onnx",
            "memory_attention.onnx",
            16_175_204,
            "bf1a708c2ee94f5a2177933a2559f568a3f760e8a18abe392fd7ebca346b4727"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-large-video-onnx-fp16",
            "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1",
            "onnx/memory_encoder.onnx",
            "memory_encoder.onnx",
            2_807_886,
            "c809462a85e2354437e705a70bbb5a74f9194e28db7443dd69f68b8ae4f4047d"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-large-video-onnx-fp16",
            "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1",
            "onnx/pointer_tpos.onnx",
            "pointer_tpos.onnx",
            34_228,
            "d32d040ad52cc4507b2978af8c151869179e3195759801267b59443b1ac9dcf5"
        ),
        sam21_file!(
            "diffusionstudio/sam2.1-large-video-onnx-fp16",
            "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1",
            "constants.json",
            "constants.json",
            9_752,
            "87e2c0e431cc3390eadf20e9d5d454037d5355f7e771ba493cd5c2dfc7516fbb"
        ),
    ],
};
pub static MODELS: &[&Model] = &[
    &EDGETAM,
    &SAM21_BASE,
    &SAM21_LARGE,
    &VITMATTE,
    &VITMATTE_FINE,
];

pub fn find(id: &str) -> Result<&'static Model> {
    MODELS
        .iter()
        .copied()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("unknown segmentation model {id:?}").into())
}
/// `VEYLVIK_MODELS_DIR`, else `$XDG_DATA_HOME/veylvik/models`, else `~/.local/share/veylvik/models`.
pub fn root() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = OVERRIDE.with(|o| o.borrow().clone()) {
        return Ok(dir);
    }
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if let Some(dir) = env("VEYLVIK_MODELS_DIR") {
        return Ok(dir);
    }
    if let Some(data) = env("XDG_DATA_HOME") {
        return Ok(data.join("veylvik/models"));
    }
    env("HOME")
        .map(|home| home.join(".local/share/veylvik/models"))
        .ok_or_else(|| "cannot find a models directory: set HOME or VEYLVIK_MODELS_DIR".into())
}
#[cfg(test)]
thread_local! {
    static OVERRIDE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}
pub fn dir(model: &Model) -> Result<PathBuf> {
    Ok(root()?.join(model.id))
}
/// True when every file is present at its pinned size (hashes were checked when installed).
pub fn installed(model: &Model) -> Result<bool> {
    let dir = dir(model)?;
    Ok(model.files.iter().all(|f| {
        std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.is_file() && m.len() == f.size)
    }))
}
/// Switched-off models stay installed but are neither offered nor loaded. Kept in
/// `<models>/settings.json` as `{"disabled": [ids]}`.
fn settings_path() -> Result<PathBuf> {
    Ok(root()?.join("settings.json"))
}
fn disabled() -> Result<Vec<String>> {
    match std::fs::read(settings_path()?) {
        Ok(bytes) => Ok(
            serde_json::from_slice::<serde_json::Value>(&bytes)?["disabled"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}
pub fn enabled(model: &Model) -> Result<bool> {
    Ok(!disabled()?.iter().any(|id| id == model.id))
}
pub fn set_enabled(model: &Model, on: bool) -> Result<()> {
    let mut list = disabled()?;
    list.retain(|id| id != model.id && find(id).is_ok());
    if !on {
        list.push(model.id.to_owned());
    }
    let path = settings_path()?;
    std::fs::create_dir_all(root()?)?;
    let partial = path.with_extension("json.partial");
    std::fs::write(
        &partial,
        serde_json::to_vec_pretty(&serde_json::json!({"disabled": list}))?,
    )?;
    std::fs::rename(partial, path)?;
    Ok(())
}
/// True when the model can be used now: installed and switched on.
pub fn ready(model: &Model) -> Result<bool> {
    Ok(installed(model)? && enabled(model)?)
}
/// Removes the model's files: only the catalogue's own names (and partial downloads) inside its
/// directory, then the directory if it is empty. Never follows anything else.
pub fn uninstall(model: &Model) -> Result<()> {
    let dir = dir(model)?;
    for file in model.files {
        for name in [file.name.to_owned(), format!("{}.partial", file.name)] {
            match std::fs::remove_file(dir.join(name)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    match std::fs::remove_dir(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) if std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some()) => {}
        result => result?,
    }
    Ok(())
}
pub fn sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
/// Downloads and verifies every missing file. `progress(done, total)` returns false to cancel.
/// Files are fetched with `curl` over HTTPS only and renamed into place after their hash matches.
pub fn install(model: &Model, progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<()> {
    let dir = dir(model)?;
    std::fs::create_dir_all(&dir)?;
    let total = model.bytes();
    let mut done = 0;
    for file in model.files {
        let target = dir.join(file.name);
        if std::fs::metadata(&target).is_ok_and(|m| m.len() == file.size)
            && sha256(&target)? == file.sha256
        {
            done += file.size;
            continue;
        }
        let partial = dir.join(format!("{}.partial", file.name));
        let _ = std::fs::remove_file(&partial);
        fetch(file, &partial, &mut |n| progress(done + n, total))?;
        let actual = sha256(&partial)?;
        if actual != file.sha256 {
            let _ = std::fs::remove_file(&partial);
            return Err(format!(
                "{} failed verification (SHA-256 {actual}); nothing was installed",
                file.name
            )
            .into());
        }
        std::fs::rename(&partial, &target)?;
        done += file.size;
        if !progress(done, total) {
            return Err("model download cancelled".into());
        }
    }
    Ok(())
}
fn fetch(file: &File, partial: &Path, progress: &mut dyn FnMut(u64) -> bool) -> Result<()> {
    let mut command = Command::new("curl");
    command
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args(["--max-filesize", &(file.size + 1024).to_string()]);
    if let Some((first, last)) = file.range {
        command.args(["--range", &format!("{first}-{last}")]);
    }
    let mut child = command
        .arg("--output")
        .arg(partial)
        .arg(file.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start curl to download the model: {e}"))?;
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr {
            let _ = stderr.take(8192).read_to_string(&mut text);
        }
        text
    });
    // Stops when the transfer stalls for a minute, or is cancelled.
    let (mut last_size, mut last_change) = (0, Instant::now());
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        let size = std::fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
        if size != last_size {
            (last_size, last_change) = (size, Instant::now());
        }
        if !progress(size) || last_change.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let errors = errors.join().unwrap_or_default();
    match status {
        Some(status) if status.success() => {
            let size = std::fs::metadata(partial)?.len();
            if size != file.size {
                return Err(format!(
                    "{} downloaded as {size} bytes, expected {}",
                    file.name, file.size
                )
                .into());
            }
            Ok(())
        }
        Some(_) => Err(format!("could not download {}: {}", file.name, errors.trim()).into()),
        None if last_change.elapsed() > Duration::from_secs(60) => {
            Err(format!("download of {} stalled; try again", file.name).into())
        }
        None => Err("model download cancelled".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_pins_every_file_by_https_revision_size_and_hash() {
        for model in MODELS {
            assert!(!model.files.is_empty());
            // Every model explains itself in the Models window's information panel.
            assert!(!model.uses.is_empty() && !model.speed.is_empty() && !model.memory.is_empty());
            assert!(!model.pros.is_empty() && !model.cons.is_empty());
            for file in model.files {
                assert!(
                    file.url.starts_with("https://huggingface.co/"),
                    "{}",
                    file.url
                );
                assert!(
                    file.url.contains("/resolve/"),
                    "pinned revision: {}",
                    file.url
                );
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
                if let Some((a, b)) = file.range {
                    assert_eq!(b - a + 1, file.size, "{}", file.name);
                }
                assert!(!file.name.contains('/'));
            }
        }
        assert!(find("edgetam").is_ok() && find("nope").is_err());
    }
    #[test]
    fn models_switch_off_and_uninstall_only_their_own_files() {
        let root = std::env::temp_dir().join(format!("veylvik-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let model = &EDGETAM;
        temp_env(&root, || {
            assert!(enabled(model).unwrap() && !installed(model).unwrap());
            set_enabled(model, false).unwrap();
            assert!(!enabled(model).unwrap() && !ready(model).unwrap());
            set_enabled(model, true).unwrap();
            assert!(enabled(model).unwrap());
            let dir = dir(model).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(model.files[0].name), b"x").unwrap();
            std::fs::write(dir.join("mine.txt"), b"keep").unwrap();
            uninstall(model).unwrap();
            assert!(!dir.join(model.files[0].name).exists());
            assert!(
                dir.join("mine.txt").exists(),
                "foreign files are never removed"
            );
            std::fs::remove_file(dir.join("mine.txt")).unwrap();
            uninstall(model).unwrap();
            assert!(!dir.exists());
        });
        let _ = std::fs::remove_dir_all(&root);
    }
    fn temp_env(root: &Path, test: impl FnOnce()) {
        // Tests read the models root through this override only.
        OVERRIDE.with(|o| *o.borrow_mut() = Some(root.to_path_buf()));
        test();
        OVERRIDE.with(|o| *o.borrow_mut() = None);
    }
}
