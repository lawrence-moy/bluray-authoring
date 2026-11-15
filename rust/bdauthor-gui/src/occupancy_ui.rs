//! Jauge d'occupation du disque, dans la barre du haut : estimation en direct (videos, audio, sous-titres, menus) comparee a la
//! capacite du disque vise ; le detail s'affiche au survol.
use crate::i18n::{tr, trf};
use crate::App;
use bdauthor_core::occupancy::{estimate, gb, Usage, VideoFacts};
use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::path::PathBuf;

thread_local! {
    /// Taille des fichiers sources (octets), lue une fois par chemin.
    static SIZES: std::cell::RefCell<HashMap<PathBuf, Option<u64>>> = std::cell::RefCell::new(HashMap::new());
}

fn file_size(p: PathBuf) -> Option<u64> {
    SIZES.with(|c| *c.borrow_mut().entry(p.clone()).or_insert_with(|| std::fs::metadata(&p).ok().map(|m| m.len())))
}

const GREEN: Color32 = Color32::from_rgb(90, 175, 110);
const AMBER: Color32 = Color32::from_rgb(230, 170, 60);
const RED: Color32 = Color32::from_rgb(225, 90, 80);

impl App {
    fn disc_usage(&self) -> Usage {
        let base = self.base_dir();
        estimate(&self.model, &|i| {
            let Some(v) = self.model.videos.get(i) else { return VideoFacts::default() };
            let duration = self.durations.get(&v.file).copied();
            let height = self.video_sizes.get(&v.file).map(|s| s.0);
            let source_kbps = match (duration, file_size(crate::editor_ui::abs_path(&base, &v.file))) {
                (Some(d), Some(b)) if d > 0.0 => Some((b as f64 * 8.0 / d / 1000.0) as u32),
                _ => None,
            };
            VideoFacts { duration, height, source_kbps }
        })
    }

    /// Jauge compacte : « BD-25 · 18.2 / 25.0 Go », barre coloree, detail au survol.
    pub(crate) fn disc_gauge(&self, ui: &mut egui::Ui) {
        if self.model.videos.is_empty() { return; }
        let u = self.disc_usage();
        let ratio = u.ratio();
        let col = if u.over_capacity() { RED } else if u.over_margin() { AMBER } else { GREEN };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(110.0, 12.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        let mut fill = rect;
        fill.set_width(rect.width() * ratio.min(1.0) as f32);
        p.rect_filled(fill, 3.0, col);
        // repere de la marge de securite
        let x = rect.left() + rect.width() * u.margin_pct as f32 / 100.0;
        p.line_segment([egui::pos2(x, rect.top() - 2.0), egui::pos2(x, rect.bottom() + 2.0)], Stroke::new(1.0_f32, ui.visuals().text_color()));
        p.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color));
        let label = trf!("{} · {} / {} GB", u.disc, gb(u.total), gb(u.capacity));
        let text = ui.label(RichText::new(label).small().color(if u.over_margin() { col } else { ui.visuals().text_color() }));
        let tip = |ui: &mut egui::Ui| {
            ui.strong(trf!("Disc usage (estimate): {} GB of {} ({} %)", gb(u.total), u.disc, format!("{:.0}", ratio * 100.0)));
            egui::Grid::new("occupation").num_columns(2).show(ui, |ui| {
                for part in &u.parts {
                    ui.label(&part.label);
                    ui.label(RichText::new(trf!("{} GB", gb(part.bytes))).monospace());
                    ui.end_row();
                }
            });
            ui.label(trf!("Safety margin: {} % of the disc ({} GB).", u.margin_pct, gb(u.usable())));
            if u.fill {
                ui.label(tr("“Aim for a bitrate that fills the disc” is on: the videos take the room left under the margin."));
            } else {
                ui.label(RichText::new(tr("The size of the encoded videos is estimated from the quality (CRF) and the picture size; the real size is known only after encoding.")).weak());
            }
            if u.unknown > 0 {
                ui.label(RichText::new(trf!("{} video(s) not read yet: not counted.", u.unknown)).color(AMBER));
            }
            if u.over_capacity() {
                ui.label(RichText::new(tr("The content does not fit on this disc: raise the CRF, use “fill the disc”, choose a larger disc or remove content.")).color(RED));
            } else if u.over_margin() {
                ui.label(RichText::new(tr("Beyond the safety margin: burning is less reliable at the edge of a disc.")).color(AMBER));
            }
            ui.label(RichText::new(tr("Disc and margin: Edit › Project… › ISO generation.")).small().weak());
        };
        resp.on_hover_ui(tip);
        text.on_hover_ui(tip);
    }
}
