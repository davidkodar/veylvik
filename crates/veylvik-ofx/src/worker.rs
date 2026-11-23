//! The helper's side: reads requests on stdin, runs plugins, answers on stdout. Bundles stay loaded and instances stay made for the helper's life, so repeated renders of one effect cost only the render.
use crate::protocol::{self, Reply, Request};
use crate::{Clip, Description, Param, Plugin, Result};
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use veylvik_interop::ofx::{Frame, Instance, Library};

type Key = (String, usize, String, u32, u32);
struct State {
    libraries: HashMap<String, &'static Library>,
    instances: HashMap<Key, Instance<'static>>,
}
impl State {
    fn library(&mut self, bundle: &str) -> Result<&'static Library> {
        if let Some(lib) = self.libraries.get(bundle) {
            return Ok(lib);
        }
        // Kept for the helper's life: instances borrow it, and unloading plugin code
        // that may still own threads is not worth the risk in a helper that exits soon.
        let lib: &'static Library =
            Box::leak(Box::new(Library::open(std::path::Path::new(bundle))?));
        self.libraries.insert(bundle.to_string(), lib);
        Ok(lib)
    }
    fn plugin(lib: &Library, bundle: &str, index: usize) -> Result<Plugin> {
        let info = lib.info(index)?;
        Ok(Plugin {
            bundle: PathBuf::from(bundle),
            index,
            identifier: info.identifier,
            version: info.version,
            label: info.label,
            grouping: info.grouping,
            description: info.description,
            contexts: info.contexts,
        })
    }
    fn answer(&mut self, request: Request, payload: Vec<f32>) -> Result<(Reply, Vec<f32>)> {
        let mut reply = Reply::default();
        match request {
            Request::List { bundle } => {
                let lib = self.library(&bundle)?;
                for index in 0..lib.len() {
                    match lib.identifier(index) {
                        Ok((_, true)) => match Self::plugin(lib, &bundle, index) {
                            Ok(p) => reply.plugins.push(p),
                            Err(e) => reply.refused.push(format!(
                                "{}: {e}",
                                lib.identifier(index).map(|i| i.0).unwrap_or_default()
                            )),
                        },
                        Ok((id, false)) => reply.refused.push(format!("{id}: not an image effect")),
                        Err(e) => reply.refused.push(e),
                    }
                }
            }
            Request::Describe {
                bundle,
                index,
                context,
            } => {
                let lib = self.library(&bundle)?;
                let plugin = Self::plugin(lib, &bundle, index)?;
                let d = lib.describe(index, &context)?;
                reply.description = Some(Description {
                    plugin,
                    context: d.context,
                    clips: d
                        .clips
                        .into_iter()
                        .map(|c| Clip {
                            name: c.name,
                            optional: c.optional,
                            mask: c.mask,
                        })
                        .collect(),
                    params: d
                        .params
                        .into_iter()
                        .map(|p| Param {
                            name: p.name,
                            kind: p.kind,
                            label: p.label,
                            hint: p.hint,
                            parent: p.parent,
                            default: p.default,
                            text: p.text,
                            min: p.min,
                            max: p.max,
                            display_min: p.display_min,
                            display_max: p.display_max,
                            options: p.options,
                            secret: p.secret,
                        })
                        .collect(),
                });
            }
            Request::Render {
                bundle,
                index,
                context,
                width,
                height,
                time,
                values,
                inputs,
            } => {
                let pixels = width as usize * height as usize * 4;
                if payload.len() != pixels * inputs.len() {
                    return Err("the pictures sent do not match the size asked for".into());
                }
                let lib = self.library(&bundle)?;
                let key = (bundle, index, context.clone(), width, height);
                if !self.instances.contains_key(&key) {
                    let instance = lib.instance(index, &context, width, height)?;
                    self.instances.insert(key.clone(), instance);
                }
                let instance = self.instances.get_mut(&key).ok_or("no instance")?;
                let frames: Vec<(&str, Frame<'_>)> = inputs
                    .iter()
                    .zip(payload.chunks_exact(pixels.max(1)))
                    .map(|(name, pixels)| {
                        (
                            name.as_str(),
                            Frame {
                                pixels,
                                width,
                                height,
                            },
                        )
                    })
                    .collect();
                let values: Vec<_> = values.into_iter().map(|(n, v)| (n, v.into())).collect();
                let out = instance.render(time, &values, &frames)?;
                reply.messages = veylvik_interop::ofx::messages();
                return Ok((reply, out));
            }
            Request::Quit => {}
        }
        reply.messages = veylvik_interop::ofx::messages();
        Ok((reply, Vec::new()))
    }
}
/// Serves requests until stdin closes or `Quit` arrives.
pub fn serve() -> Result<()> {
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());
    let mut state = State {
        libraries: HashMap::new(),
        instances: HashMap::new(),
    };
    while let Some((request, payload)) = protocol::read::<Request>(&mut input)? {
        let quit = matches!(request, Request::Quit);
        let (reply, pixels) = state.answer(request, payload).unwrap_or_else(|error| {
            let messages = veylvik_interop::ofx::messages();
            (
                Reply {
                    error: Some(error),
                    messages,
                    ..Default::default()
                },
                Vec::new(),
            )
        });
        protocol::write(&mut output, &reply, &[&pixels])?;
        if quit {
            break;
        }
    }
    // Instances are destroyed before their libraries, which are never unloaded.
    state.instances.clear();
    Ok(())
}
