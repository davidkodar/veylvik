use crate::{Result, assets};
use std::path::Path;
use veylvik_document::{Composition, Time};
pub fn run(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err("render-video needs FILE OUTPUT.mp4 [--codec h264|hevc10] [--size W H] [--fps N D] [--range FIRST LAST] [--quality draft|high|master] [--allow-software] [--no-audio] [--audio FILE] [--audio-gain DB]".into());
    }
    let doc = Composition::load(Path::new(&args[0]))?;
    let base = assets::document_base(Path::new(&args[0]))?;
    let mut settings = assets::video_export::Settings::for_document(&doc);
    let mut seen = std::collections::BTreeSet::new();
    let mut i = 2;
    while i < args.len() {
        let option = &args[i];
        if !seen.insert(option) {
            return Err(format!("Repeated option {option}").into());
        }
        let value = |offset: usize| {
            args.get(i + offset)
                .ok_or_else(|| format!("Missing value for {option}"))
        };
        match option.as_str() {
            "--codec" => {
                settings.codec = serde_json::from_value(serde_json::json!(value(1)?))?;
                i += 2;
            }
            "--quality" => {
                settings.quality = serde_json::from_value(serde_json::json!(value(1)?))?;
                i += 2;
            }
            "--size" => {
                settings.width = value(1)?.parse()?;
                settings.height = value(2)?.parse()?;
                i += 3;
            }
            "--fps" => {
                settings.frame_rate = Time::new(value(1)?.parse()?, value(2)?.parse()?)?;
                i += 3;
            }
            "--range" => {
                settings.start = value(1)?.parse()?;
                settings.end = value(2)?.parse()?;
                i += 3;
            }
            "--allow-software" => {
                settings.allow_software = true;
                i += 1;
            }
            "--no-audio" => {
                settings.audio.mute = true;
                i += 1;
            }
            "--audio" => {
                settings.audio.music = Some(std::path::PathBuf::from(value(1)?));
                i += 2;
            }
            "--audio-gain" => {
                settings.audio.music_gain_db = value(1)?.parse()?;
                i += 2;
            }
            _ => return Err(format!("Unknown video export option {option}").into()),
        }
    }
    settings.audio.base = Some(base.clone());
    settings.times(&doc)?;
    let renderer = assets::runtime::renderer()?;
    let plan = veylvik_render::Plan::compile(&doc)?;
    let inputs = assets::load_inputs(&plan, &base, renderer.budget_bytes)?;
    // One updating line on a terminal; otherwise phase changes plus about one line a second.
    use std::io::{IsTerminal, Write};
    let terminal = std::io::stderr().is_terminal();
    let (mut phase, mut printed) = (String::new(), std::time::Instant::now());
    let result = assets::video_export::render(
        &renderer,
        &doc,
        &inputs,
        Path::new(&args[1]),
        &settings,
        |p| {
            let line = format!("{} · {} · {}/{}", p.backend, p.phase, p.completed, p.total);
            let changed = p.phase != phase;
            if terminal {
                eprint!("\r\x1b[2K{line}");
                let _ = std::io::stderr().flush();
            } else if changed || printed.elapsed() >= std::time::Duration::from_secs(1) {
                eprintln!("{line}");
                printed = std::time::Instant::now();
            }
            phase = p.phase;
            true
        },
    )?;
    if terminal {
        eprintln!();
    }
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
