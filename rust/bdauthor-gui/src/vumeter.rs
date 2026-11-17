//! Vumetre (crete-metre) : une barre par canal en dB pleine echelle (dBFS), montee immediate, descente progressive, maintien
//! du pic et voyant de saturation.
use crate::i18n::tr;
use eframe::egui;
use std::time::Instant;

/// Bas de l'echelle (dBFS).
const FLOOR_DB: f32 = -60.0;
/// Vitesse de descente des barres (dB par seconde).
const FALL_DB_S: f32 = 24.0;
/// Duree du maintien du pic et du voyant de saturation (s).
const HOLD_S: f32 = 1.5;

#[derive(Default)]
pub struct VuMeter {
    /// niveau affiche (dB), pic maintenu (dB) et depuis quand, derniere saturation
    level: Vec<f32>,
    hold: Vec<(f32, Instant)>,
    clip: Vec<Option<Instant>>,
    last: Option<Instant>,
}

fn db(a: f32) -> f32 { if a <= 1e-6 { FLOOR_DB } else { (20.0 * a.log10()).max(FLOOR_DB) } }

/// Nom des canaux selon leur nombre (ordre de ffmpeg et des sorties audio).
fn channel_names(n: usize) -> Vec<String> {
    let names: &[&str] = match n {
        1 => &["M"],
        2 => &["L", "R"],
        6 => &["L", "R", "C", "LFE", "Ls", "Rs"],
        8 => &["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs"],
        _ => &[],
    };
    if names.is_empty() { (1..=n).map(|k| k.to_string()).collect() } else { names.iter().map(|s| tr(s).to_string()).collect() }
}

impl VuMeter {
    /// Ajoute les cretes relevees depuis l'appel precedent (vide : rien ne joue, les barres retombent).
    pub fn update(&mut self, peaks: &[f32]) {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |t| now.duration_since(t).as_secs_f32()).min(0.5);
        self.last = Some(now);
        if !peaks.is_empty() && peaks.len() != self.level.len() {
            self.level = vec![FLOOR_DB; peaks.len()];
            self.hold = vec![(FLOOR_DB, now); peaks.len()];
            self.clip = vec![None; peaks.len()];
        }
        for k in 0..self.level.len() {
            let p = peaks.get(k).copied().unwrap_or(0.0);
            let v = db(p);
            self.level[k] = v.max(self.level[k] - FALL_DB_S * dt);
            if v >= self.hold[k].0 || now.duration_since(self.hold[k].1).as_secs_f32() > HOLD_S { self.hold[k] = (v, now); }
            if p >= 0.999 { self.clip[k] = Some(now); }
        }
    }

    /// Les barres bougent encore (il faut redessiner).
    pub fn active(&self) -> bool { self.level.iter().any(|l| *l > FLOOR_DB) }

    /// Dessine les barres horizontales sur `width` points.
    pub fn ui(&self, ui: &mut egui::Ui, width: f32) {
        if self.level.is_empty() { return; }
        let names = channel_names(self.level.len());
        let bar_h = if self.level.len() > 2 { 6.0 } else { 9.0 };
        let label_w = 30.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, self.level.len() as f32 * (bar_h + 2.0) + 12.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let vis = ui.visuals();
        let x0 = rect.left() + label_w;
        let w = rect.width() - label_w - 44.0;
        let x_of = |d: f32| x0 + (d - FLOOR_DB) / -FLOOR_DB * w;
        let now = Instant::now();
        for (k, lvl) in self.level.iter().enumerate() {
            let y = rect.top() + k as f32 * (bar_h + 2.0);
            let row = egui::Rect::from_min_size(egui::pos2(x0, y), egui::vec2(w, bar_h));
            painter.text(egui::pos2(rect.left(), y + bar_h / 2.0), egui::Align2::LEFT_CENTER, &names[k], egui::FontId::proportional(10.0), vis.weak_text_color());
            painter.rect_filled(row, 1.0, vis.extreme_bg_color);
            // vert jusqu'a -18 dB, jaune jusqu'a -6 dB, rouge au-dela
            for (from, to, col) in [(FLOOR_DB, -18.0, egui::Color32::from_rgb(80, 190, 90)), (-18.0, -6.0, egui::Color32::from_rgb(225, 200, 60)), (-6.0, 0.0, egui::Color32::from_rgb(225, 80, 60))] {
                let top = lvl.min(to);
                if top > from { painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x_of(from), y), egui::pos2(x_of(top), y + bar_h)), 0.0, col); }
            }
            let (h, _) = self.hold[k];
            if h > FLOOR_DB { painter.line_segment([egui::pos2(x_of(h), y), egui::pos2(x_of(h), y + bar_h)], egui::Stroke::new(2.0_f32, vis.strong_text_color())); }
            // voyant de saturation (amplitude pleine echelle atteinte)
            let clipped = self.clip[k].is_some_and(|t| now.duration_since(t).as_secs_f32() < HOLD_S);
            let led = egui::Rect::from_min_size(egui::pos2(x0 + w + 4.0, y), egui::vec2(bar_h, bar_h));
            painter.rect_filled(led, 2.0, if clipped { egui::Color32::from_rgb(235, 50, 40) } else { vis.extreme_bg_color });
            painter.text(egui::pos2(led.right() + 4.0, y + bar_h / 2.0), egui::Align2::LEFT_CENTER,
                if h > FLOOR_DB { format!("{h:.0}") } else { "-∞".into() }, egui::FontId::monospace(10.0), vis.weak_text_color());
        }
        // graduations
        let y = rect.top() + self.level.len() as f32 * (bar_h + 2.0) + 1.0;
        for d in [-60.0, -40.0, -30.0, -20.0, -12.0, -6.0, -3.0, 0.0] {
            let x = x_of(d);
            painter.line_segment([egui::pos2(x, y), egui::pos2(x, y + 3.0)], egui::Stroke::new(1.0_f32, vis.weak_text_color()));
            painter.text(egui::pos2(x, y + 3.0), egui::Align2::CENTER_TOP, format!("{d:.0}"), egui::FontId::proportional(8.5), vis.weak_text_color());
        }
    }
}
