//! Switches that live in menus and panels but are remembered in preferences: thumbnails on nodes, layer bars, the viewer aids and editing several nodes at once. Preferences set them at start and when its window is applied; flipping one in the interface saves it.
use crate::{Studio, preferences::Aids};

impl Studio {
    fn aids(&self) -> Aids {
        let o = &self.overlays;
        Aids {
            safe: o.safe,
            centre: o.centre,
            thirds: o.thirds,
            grid: o.grid,
            rulers: o.rulers,
            readout: o.readout,
        }
    }
    /// Set the live switches from preferences.
    pub(crate) fn recall(&mut self) {
        let p = &self.preferences.values;
        self.graph.show_thumbnails = p.node_thumbnails;
        self.tools.spread = p.spread_edits;
        self.layer_bars.hidden = !p.layer_bars;
        let o = &mut self.overlays;
        (o.safe, o.centre, o.thirds, o.grid, o.rulers, o.readout) = (
            p.aids.safe,
            p.aids.centre,
            p.aids.thirds,
            p.aids.grid,
            p.aids.rulers,
            p.aids.readout,
        );
        self.recalled = true;
    }
    /// Hand settings to the parts that are rebuilt with each project, every frame.
    fn push_settings(&mut self) {
        let p = &self.preferences.values;
        self.project.document.set_history_limits(
            p.undo_steps as usize,
            p.undo_memory_mib as usize * 1024 * 1024,
        );
        self.composition_controls.defaults =
            veylvik_document::Time::new(p.new_rate.0, p.new_rate.1)
                .ok()
                .map(|rate| (p.new_width, p.new_height, rate, p.new_frames));
        let insets = [p.safe_action, p.safe_title]
            .map(|percent| (100 - percent.clamp(50, 100)) as f32 / 200.0);
        self.overlays.insets = insets;
        self.text_viewer.insets = insets;
    }
    /// Save a switch that was flipped in the interface. Called once a frame.
    pub(crate) fn remember_tick(&mut self) {
        self.push_settings();
        if !self.recalled {
            self.recall();
            return;
        }
        let aids = self.aids();
        let p = &mut self.preferences.values;
        let live = (
            self.graph.show_thumbnails,
            self.tools.spread,
            !self.layer_bars.hidden,
            aids,
        );
        if live == (p.node_thumbnails, p.spread_edits, p.layer_bars, p.aids) {
            return;
        }
        (p.node_thumbnails, p.spread_edits, p.layer_bars, p.aids) = live;
        if let Err(error) = self.preferences.persist() {
            self.fail(format!("Preferences: {error}"));
        }
    }
}
