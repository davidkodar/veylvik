//! The OpenFX effect's inspector: a catalogue of the plugins found on this machine, listed and described by a helper on a background thread so the interface never waits on plugin code, and controls built from the chosen plugin's own description.
use crate::nodes::InspectCx;
use eframe::egui;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use veylvik_document::Scalar;
use veylvik_document::nodes::{OFX_PORTS, OfxContext, OfxEffect, OfxSetting};
use veylvik_ofx::{Description, Plugin};

type Found<T> = Option<Result<T, String>>;
#[derive(Default)]
struct Catalogue {
    plugins: Found<Vec<Plugin>>,
    descriptions: HashMap<(String, String), Result<Description, String>>,
    asked: std::collections::HashSet<(String, String)>,
    listing: bool,
}
enum Job {
    List,
    Describe(Plugin),
}
struct Shared {
    catalogue: Arc<Mutex<Catalogue>>,
    jobs: Mutex<mpsc::Sender<(Job, egui::Context)>>,
}
fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| {
        let catalogue = Arc::new(Mutex::new(Catalogue::default()));
        let (jobs, todo) = mpsc::channel::<(Job, egui::Context)>();
        let into = catalogue.clone();
        std::thread::spawn(move || {
            let mut worker = veylvik_io::ofx::program().map(|exe| veylvik_ofx::Worker::new(&exe));
            while let Ok((job, ctx)) = todo.recv() {
                let Some(worker) = worker.as_mut() else {
                    if let Ok(mut c) = into.lock() {
                        c.plugins =
                            Some(Err("OpenFX needs the veylvik program beside Studio".into()));
                    }
                    ctx.request_repaint();
                    continue;
                };
                match job {
                    Job::List => {
                        let mut plugins = Vec::new();
                        for bundle in veylvik_ofx::bundles(&veylvik_ofx::folders()) {
                            if let Ok((found, _)) = worker.list(&bundle) {
                                plugins.extend(found.into_iter().filter(|p| p.context().is_some()));
                            }
                        }
                        plugins
                            .sort_by(|a, b| (&a.grouping, &a.label).cmp(&(&b.grouping, &b.label)));
                        // A plugin offered in several versions is listed once, newest first.
                        plugins
                            .dedup_by(|b, a| a.identifier == b.identifier && a.bundle == b.bundle);
                        if let Ok(mut c) = into.lock() {
                            c.plugins = Some(Ok(plugins));
                            c.listing = false;
                        }
                    }
                    Job::Describe(plugin) => {
                        let context = plugin.context().unwrap_or(veylvik_ofx::FILTER);
                        let found = worker.describe(&plugin, context);
                        if let Ok(mut c) = into.lock() {
                            c.descriptions.insert(
                                (
                                    plugin.bundle.to_string_lossy().into_owned(),
                                    plugin.identifier.clone(),
                                ),
                                found,
                            );
                        }
                    }
                }
                ctx.request_repaint();
            }
        });
        Shared {
            catalogue,
            jobs: Mutex::new(jobs),
        }
    })
}
fn ask(job: Job, ctx: &egui::Context) {
    if let Ok(jobs) = shared().jobs.lock() {
        let _ = jobs.send((job, ctx.clone()));
    }
}
/// The plugins found, once listed (asks for the list the first time).
fn plugins(ctx: &egui::Context) -> Found<Vec<Plugin>> {
    let mut c = shared().catalogue.lock().ok()?;
    if c.plugins.is_none() && !c.listing {
        c.listing = true;
        drop(c);
        ask(Job::List, ctx);
        return None;
    }
    c.plugins.clone()
}
/// A plugin's description, once read (asks for it the first time).
fn description(ctx: &egui::Context, effect: &OfxEffect) -> Found<Description> {
    let key = (effect.bundle.clone(), effect.plugin.clone());
    let mut c = shared().catalogue.lock().ok()?;
    if let Some(found) = c.descriptions.get(&key) {
        return Some(found.clone());
    }
    let plugin = c
        .plugins
        .as_ref()?
        .as_ref()
        .ok()?
        .iter()
        .find(|p| p.bundle.to_string_lossy() == effect.bundle && p.identifier == effect.plugin)?
        .clone();
    if c.asked.insert(key) {
        drop(c);
        ask(Job::Describe(plugin), ctx);
    }
    None
}
/// Which plugin clip each port feeds: the plugin's picture inputs in order on source,
/// input 2 and input 3, its mask on mask.
fn clips(description: &Description) -> Vec<String> {
    let mut clips = vec![String::new(); OFX_PORTS.len()];
    let (masks, pictures): (Vec<_>, Vec<_>) = description.inputs().partition(|c| c.mask);
    for (slot, clip) in clips.iter_mut().take(3).zip(pictures) {
        *slot = clip.name.clone();
    }
    if let Some(mask) = masks.first() {
        clips[3] = mask.name.clone();
    }
    clips
}
fn context_of(plugin: &Plugin) -> OfxContext {
    match plugin.context() {
        Some(veylvik_ofx::GENERAL) => OfxContext::General,
        Some(veylvik_ofx::GENERATOR) => OfxContext::Generator,
        _ => OfxContext::Filter,
    }
}
fn chooser(ui: &mut egui::Ui, effect: &mut OfxEffect, cx: &mut InspectCx<'_>) {
    let id = egui::Id::new(("ofx-search", cx.node_id));
    let mut search: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    match plugins(ui.ctx()) {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Looking for OpenFX plugins…");
            });
        }
        Some(Err(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        Some(Ok(plugins)) if plugins.is_empty() => {
            ui.label("No OpenFX plugins found. Plugins are looked for in OFX_PLUGIN_PATH, /usr/OFX/Plugins and /usr/local/OFX/Plugins.");
        }
        Some(Ok(plugins)) => {
            ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search plugins"));
            let wanted = search.to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .id_salt(id)
                .show(ui, |ui| {
                    let mut group = None;
                    for plugin in plugins.iter().filter(|p| {
                        wanted.is_empty()
                            || p.label.to_lowercase().contains(&wanted)
                            || p.grouping.to_lowercase().contains(&wanted)
                    }) {
                        if group != Some(&plugin.grouping) {
                            group = Some(&plugin.grouping);
                            ui.label(
                                egui::RichText::new(if plugin.grouping.is_empty() {
                                    "Other"
                                } else {
                                    &plugin.grouping
                                })
                                .strong(),
                            );
                        }
                        let picked = ui.selectable_label(false, &plugin.label).on_hover_text(
                            if plugin.description.is_empty() {
                                &plugin.identifier
                            } else {
                                &plugin.description
                            },
                        );
                        if picked.clicked() {
                            effect.bundle = plugin.bundle.to_string_lossy().into_owned();
                            effect.plugin = plugin.identifier.clone();
                            effect.context = context_of(plugin);
                            effect.other_frames = plugin.temporal;
                            effect.clips.clear();
                            effect.settings.clear();
                            cx.dirty = true;
                            search.clear();
                        }
                    }
                });
        }
    }
    ui.data_mut(|d| d.insert_temp(id, search));
}
/// The numbers a setting holds now, or its default.
fn numbers(
    effect: &OfxEffect,
    param: &veylvik_ofx::Param,
    time: veylvik_document::Time,
) -> (Vec<f64>, bool) {
    match effect.settings.get(&param.name) {
        Some(OfxSetting::Numbers(n)) => {
            let keyed = n.iter().any(|s| s.keys().is_some());
            (
                n.iter()
                    .map(|s| s.at(time).map_or(0.0, f64::from))
                    .collect(),
                keyed,
            )
        }
        _ => (param.default.clone(), false),
    }
}
/// Stores `values`: as constants, or, when the setting is keyed, as keys at `time`;
/// whether anything changed.
fn set(
    effect: &mut OfxEffect,
    name: &str,
    values: &[f64],
    keyed: bool,
    time: veylvik_document::Time,
) -> bool {
    let mut scalars = match effect.settings.get(name) {
        Some(OfxSetting::Numbers(n)) if keyed => n.clone(),
        _ => Vec::new(),
    };
    if keyed {
        scalars.resize(values.len(), Scalar::Constant { value: 0.0 });
        for (scalar, v) in scalars.iter_mut().zip(values) {
            if scalar.set_key_at(time, *v as f32).is_err() {
                return false;
            }
        }
    } else {
        scalars = values
            .iter()
            .map(|v| Scalar::Constant { value: *v as f32 })
            .collect();
    }
    effect
        .settings
        .insert(name.to_string(), OfxSetting::Numbers(scalars));
    true
}
/// What the key button does: removes the key at `time`, or adds one with `values`.
fn toggle_key(
    effect: &mut OfxEffect,
    name: &str,
    values: &[f64],
    time: veylvik_document::Time,
) -> bool {
    if !key_here(effect, name, time) {
        let was_constant = !matches!(
            effect.settings.get(name),
            Some(OfxSetting::Numbers(n)) if n.iter().any(|s| s.keys().is_some())
        );
        if !set(effect, name, values, true, time) {
            return false;
        }
        // A setting's first key takes the curve the artist prefers for first keys.
        if let Some(OfxSetting::Numbers(n)) = effect.settings.get_mut(name) {
            for scalar in n.iter_mut() {
                crate::autokey::first_key(scalar, was_constant);
            }
        }
        return true;
    }
    let Some(OfxSetting::Numbers(n)) = effect.settings.get_mut(name) else {
        return false;
    };
    for scalar in n.iter_mut() {
        let _ = scalar.remove_key_at(time);
    }
    true
}
/// Whether any channel of a setting has a key exactly at `time`.
fn key_here(effect: &OfxEffect, name: &str, time: veylvik_document::Time) -> bool {
    let Some(OfxSetting::Numbers(n)) = effect.settings.get(name) else {
        return false;
    };
    n.iter().any(|s| {
        s.keys().is_some_and(|keys| {
            keys.iter()
                .any(|k| k.time.compare(time).ok() == Some(std::cmp::Ordering::Equal))
        })
    })
}
/// The key button: adds a key with the values shown, or removes the key at the playhead.
fn key_button(
    ui: &mut egui::Ui,
    effect: &mut OfxEffect,
    name: &str,
    values: &[f64],
    keyed: bool,
    cx: &mut InspectCx<'_>,
) {
    // Key times of all channels, so the strip steps between any of them.
    let mut times: Vec<veylvik_document::Time> = match effect.settings.get(name) {
        Some(OfxSetting::Numbers(n)) => n
            .iter()
            .filter_map(|s| s.keys())
            .flatten()
            .map(|k| k.time)
            .collect(),
        _ => Vec::new(),
    };
    times.sort_by(|a, b| a.compare(*b).unwrap_or(std::cmp::Ordering::Equal));
    times.dedup_by(|a, b| a.compare(*b).is_ok_and(|o| o.is_eq()));
    if crate::animation::key_strip(ui, &times, keyed, cx.time).is_some()
        && toggle_key(effect, name, values, cx.time)
    {
        // Keys from a key button are the artist's own, not a try-out.
        crate::autokey::asked();
        cx.dirty = true;
    }
}
fn range(param: &veylvik_ofx::Param, k: usize) -> std::ops::RangeInclusive<f64> {
    let pick = |a: &[f64], b: &[f64], fallback: f64| {
        a.get(k)
            .or(b.get(k))
            .copied()
            .filter(|v| v.abs() < 1e9)
            .unwrap_or(fallback)
    };
    let low = pick(&param.display_min, &param.min, 0.0);
    let high = pick(&param.display_max, &param.max, 1.0);
    if low < high { low..=high } else { 0.0..=1.0 }
}
fn setting(
    ui: &mut egui::Ui,
    effect: &mut OfxEffect,
    param: &veylvik_ofx::Param,
    cx: &mut InspectCx<'_>,
) {
    let kind = param.kind.trim_start_matches("OfxParamType");
    if param.secret || matches!(kind, "Group" | "Page" | "PushButton" | "Custom" | "Camera") {
        return;
    }
    if kind == "Parametric" {
        crate::ofx_curves::editor(ui, effect, param, cx);
        return;
    }
    ui.horizontal(|ui| {
        ui.label(&param.label)
            .on_hover_text(if param.hint.is_empty() {
                &param.name
            } else {
                &param.hint
            });
        if kind == "String" || kind == "StrChoice" {
            let mut text = match effect.settings.get(&param.name) {
                Some(OfxSetting::Text(t)) => t.clone(),
                _ => param.text.clone(),
            };
            if ui.text_edit_singleline(&mut text).changed() {
                effect
                    .settings
                    .insert(param.name.clone(), OfxSetting::Text(text));
                cx.dirty = true;
            }
            return;
        }
        let (mut values, keyed) = numbers(effect, param, cx.time);
        // The host tells plugins that switches and choices do not animate.
        if !matches!(kind, "Boolean" | "Choice") {
            key_button(ui, effect, &param.name, &values, keyed, cx);
        }
        let before = values.clone();
        match kind {
            "Boolean" => {
                let mut on = values.first().copied().unwrap_or(0.0) != 0.0;
                ui.checkbox(&mut on, "");
                values = vec![f64::from(u8::from(on))];
            }
            "Choice" => {
                let mut at = values.first().copied().unwrap_or(0.0).max(0.0) as usize;
                egui::ComboBox::from_id_salt((cx.node_id, &param.name))
                    .selected_text(param.options.get(at).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (k, option) in param.options.iter().enumerate() {
                            ui.selectable_value(&mut at, k, option);
                        }
                    });
                values = vec![at as f64];
            }
            "RGB" | "RGBA" => {
                let mut colour = [0.0f32, 0.0, 0.0, 1.0];
                for (c, v) in colour.iter_mut().zip(&values) {
                    *c = *v as f32;
                }
                let mut rgba =
                    egui::Rgba::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]);
                egui::color_picker::color_edit_button_rgba(
                    ui,
                    &mut rgba,
                    egui::color_picker::Alpha::OnlyBlend,
                );
                let picked = rgba.to_rgba_unmultiplied();
                values = picked
                    .iter()
                    .take(values.len().max(3))
                    .map(|v| f64::from(*v))
                    .collect();
                for (k, v) in values.iter_mut().enumerate() {
                    ui.add(
                        egui::DragValue::new(v)
                            .speed(0.01)
                            .range(range(param, k))
                            .max_decimals(3),
                    );
                }
            }
            "Double" | "Integer" if values.len() == 1 => {
                let ints = kind == "Integer";
                let slider = egui::Slider::new(&mut values[0], range(param, 0))
                    .clamping(egui::SliderClamping::Never);
                ui.add(if ints { slider.integer() } else { slider });
            }
            _ => {
                for (k, v) in values.iter_mut().enumerate() {
                    let drag = egui::DragValue::new(v).speed(0.01).range(range(param, k));
                    ui.add(if kind.starts_with("Integer") {
                        drag.max_decimals(0)
                    } else {
                        drag.max_decimals(3)
                    });
                }
            }
        }
        if values != before && set(effect, &param.name, &values, keyed, cx.time) {
            cx.dirty = true;
        }
        if effect.settings.contains_key(&param.name)
            && ui
                .small_button("Default")
                .on_hover_text("Back to the plugin's default")
                .clicked()
        {
            effect.settings.remove(&param.name);
            cx.dirty = true;
        }
    });
}
/// The inspector for an OpenFX effect.
pub(crate) fn inspect(effect: &mut OfxEffect, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
    let changing = egui::Id::new(("ofx-change", cx.node_id));
    let mut change: bool = ui.data(|d| d.get_temp(changing)).unwrap_or(false);
    if effect.plugin.is_empty() || change {
        ui.label("Choose an OpenFX plugin:");
        let before = effect.plugin.clone();
        chooser(ui, effect, cx);
        if effect.plugin != before {
            change = false;
        }
        if !effect.plugin.is_empty() && ui.button("Keep the current plugin").clicked() {
            change = false;
        }
        ui.data_mut(|d| d.insert_temp(changing, change));
        return;
    }
    let ctx = ui.ctx().clone();
    // The plugin's label once the list is in, its identifier until then.
    let label = plugins(&ctx)
        .and_then(|p| p.ok())
        .and_then(|p| {
            p.into_iter()
                .find(|p| p.identifier == effect.plugin)
                .map(|p| p.label)
        })
        .unwrap_or_else(|| effect.plugin.clone());
    ui.horizontal(|ui| {
        ui.strong(label);
        if ui.small_button("Change…").clicked() {
            ui.data_mut(|d| d.insert_temp(changing, true));
        }
    });
    match description(&ctx, effect) {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading the plugin's settings…");
            });
        }
        Some(Err(e)) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("This plugin cannot be used: {e}"),
            );
        }
        Some(Ok(d)) => {
            let wired = clips(&d);
            if effect.clips != wired {
                effect.clips = wired;
                cx.dirty = true;
            }
            let names: Vec<String> = d.inputs().map(|c| c.name.clone()).collect();
            if !names.is_empty() {
                ui.label(egui::RichText::new(format!("Inputs: {}", names.join(", "))).weak());
            }
            for param in &d.params {
                setting(ui, effect, param, cx);
            }
        }
    }
}
/// What an OpenFX effect's port feeds, named by the plugin's own clip ("A", "Mask"),
/// or that it feeds nothing; None for other nodes (they keep their port names).
pub(crate) fn port_name(operation: &veylvik_document::Operation, port: &str) -> Option<String> {
    let veylvik_document::Operation::OfxEffect(effect) = operation else {
        return None;
    };
    if effect.plugin.is_empty() || effect.clips.is_empty() {
        return None;
    }
    let at = OFX_PORTS.iter().position(|p| *p == port)?;
    Some(match effect.clips.get(at).map(String::as_str) {
        Some(clip) if !clip.is_empty() => clip.to_string(),
        _ => format!("{} (not used)", crate::graph_port_label(port)),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ports_are_named_by_the_clips_they_feed() {
        let mut effect = OfxEffect {
            bundle: "/b.ofx.bundle".into(),
            plugin: "net.sf.openfx.MergePlugin".into(),
            context: OfxContext::Filter,
            clips: vec!["Source".into(), "A".into(), String::new(), "Mask".into()],
            other_frames: false,
            settings: Default::default(),
        };
        let name = |e: &OfxEffect, p: &str| {
            port_name(&veylvik_document::Operation::OfxEffect(e.clone()), p)
        };
        assert_eq!(name(&effect, "source").as_deref(), Some("Source"));
        assert_eq!(name(&effect, "input_2").as_deref(), Some("A"));
        assert_eq!(
            name(&effect, "input_3").as_deref(),
            Some("Input 3 (not used)")
        );
        assert_eq!(name(&effect, "mask").as_deref(), Some("Mask"));
        // Before a plugin is chosen, the ports keep their own names.
        effect.plugin.clear();
        assert_eq!(name(&effect, "source"), None);
    }
    #[test]
    fn a_setting_is_animated_by_keys_at_the_playhead_and_back_to_constant() {
        use veylvik_document::nodes::OfxValue;
        let rate = veylvik_document::Time { ticks: 24, rate: 1 };
        let at = |f| veylvik_document::Time::at_frame(f, rate).unwrap();
        let mut effect = OfxEffect {
            bundle: String::new(),
            plugin: "p".into(),
            context: OfxContext::Filter,
            clips: Vec::new(),
            other_frames: false,
            settings: Default::default(),
        };
        // An edit before animating is a constant.
        assert!(set(&mut effect, "gain", &[2.0, 3.0], false, at(0)));
        assert!(!key_here(&effect, "gain", at(0)));
        // The key button keys every channel; an edit elsewhere adds a key there.
        assert!(toggle_key(&mut effect, "gain", &[2.0, 3.0], at(0)));
        assert!(key_here(&effect, "gain", at(0)));
        assert!(set(&mut effect, "gain", &[4.0, 5.0], true, at(10)));
        assert_eq!(
            effect.values_at(at(5)).unwrap()[0].1,
            OfxValue::Numbers(vec![3.0, 4.0])
        );
        // Removing both keys leaves a constant at the last value removed.
        assert!(toggle_key(&mut effect, "gain", &[], at(10)));
        assert!(toggle_key(&mut effect, "gain", &[], at(0)));
        let Some(OfxSetting::Numbers(n)) = effect.settings.get("gain") else {
            panic!()
        };
        assert!(n.iter().all(|s| s.keys().is_none()), "{n:?}");
        assert_eq!(
            effect.values_at(at(7)).unwrap()[0].1,
            OfxValue::Numbers(vec![2.0, 3.0])
        );
    }
}
