//! `veylvik link`: how a host editor finds this Veylvik, asks for a project linked to one of its clips, and gets the picture back. Every answer is one line of JSON.
use crate::{Result, assets::link};
use std::path::Path;
pub fn run(args: &[String]) -> Result<()> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["info"] => {
            // Studio is installed beside this program; a host starts it with the project's path.
            let studio = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("veylvik-studio")))
                .filter(|path| path.is_file());
            println!(
                "{}",
                serde_json::json!({"application":"Veylvik","version":env!("CARGO_PKG_VERSION"),"protocol":link::PROTOCOL,
                    "studio":studio,"max_handles":link::MAX_HANDLES,"result":"ProRes 4444 with straight alpha, in a .mov"})
            );
        }
        ["new", request, project] => {
            let made = link::create(Path::new(request), Path::new(project))?;
            println!(
                "{}",
                serde_json::json!({"project":project,"link":link::path_for(Path::new(project)),"frames":made.before + made.clip_frames() + made.after,
                    "before":made.before,"after":made.after,"clip_frames":made.clip_frames()})
            );
        }
        ["show", project] => match link::load(Path::new(project))? {
            Some(found) => println!("{}", serde_json::to_string(&found)?),
            None => println!("null"),
        },
        ["send", project] | ["send", project, "--socket", _] => {
            let mut printed = std::time::Instant::now();
            let socket = args.get(3).map(Path::new);
            let sent = link::send(Path::new(project), socket, |p| {
                if printed.elapsed() >= std::time::Duration::from_secs(1) {
                    eprintln!("{} · {} · {}/{}", p.backend, p.phase, p.completed, p.total);
                    printed = std::time::Instant::now();
                }
                true
            })?;
            println!("{}", serde_json::to_string(&sent)?);
        }
        _ => {
            return Err("link needs: info | new REQUEST.json PROJECT.veylvik | show PROJECT.veylvik | send PROJECT.veylvik [--socket PATH]".into());
        }
    }
    Ok(())
}
