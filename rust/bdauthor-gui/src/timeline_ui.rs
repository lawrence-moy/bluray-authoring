//! Barre de temps d'une video, commune a l'editeur de chapitres et au selecteur d'image : graduations, vignettes de toute la
//! video (calculees en arriere-plan), zoom (molette) et defilement (Maj + molette), tete de lecture, aperçu agrandi au survol.
//! Les reperes propres a chaque fenetre (chapitres, images cles...) sont dessines par l'appelant ; la souris (cliquer / glisser)
//! reste aussi a l'appelant, qui connait ce qu'elle doit faire.
use crate::chapters_ui::{tc, tex_of};
use crate::i18n::tr;
use bdauthor_core::framesrc::{scaled_height, strip_thumb_size, Strip};
use eframe::egui::{self, Color32, Rect, Stroke};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Couleur de la tete de lecture.
pub const HEAD: Color32 = Color32::from_rgb(255, 140, 58);
/// Largeur des vignettes calculees.
const THUMB_W: u32 = 160;
/// Hauteur de la bande de vignettes et de la ligne de graduations, a l'ecran.
const THUMB_H: f32 = 64.0;
const TOP: f32 = 18.0;

pub struct Timeline {
    /// zoom (1 = toute la video) et instant du bord gauche de la vue
    pub zoom: f64,
    pub t0: f64,
    strip: Option<Strip>,
    tex: Vec<Option<egui::TextureHandle>>,
    /// nombre de vignettes et prefixe du nom de leurs textures
    n: usize,
    name: &'static str,
    /// video et taille d'affichage (pour calculer des vignettes a une autre echelle)
    file: Option<PathBuf>,
    view: (u32, u32),
    /// vignettes a l'echelle de la vue zoomee : en service, en cours de calcul, et vue attendue stable avant de les calculer
    detail: Option<Detail>,
    loading: Option<Detail>,
    want: Option<((f64, f64), Instant)>,
}

/// Vignettes d'une plage [a, b] de la video, a l'echelle de la vue (une par case de la frise).
struct Detail {
    a: f64,
    b: f64,
    n: usize,
    strip: Option<Strip>,
    tex: Vec<Option<egui::TextureHandle>>,
}

impl Detail {
    /// Duree que represente une vignette.
    fn cell_dt(&self) -> f64 { (self.b - self.a) / self.n as f64 }
    fn index(&self, t: f64) -> Option<usize> {
        if t < self.a || t >= self.b { return None; }
        Some((((t - self.a) / (self.b - self.a) * self.n as f64) as usize).min(self.n - 1))
    }
    /// Convient a la vue [t0, t0 + span] avec des cases de `cell_dt` secondes : la plage la contient, a peu pres a la meme echelle.
    fn fits(&self, t0: f64, span: f64, cell_dt: f64) -> bool {
        self.a <= t0 + 1e-6 && t0 + span <= self.b + 1e-6 && (self.cell_dt() / cell_dt - 1.0).abs() < 0.2
    }
    /// Recupere les vignettes arrivees ; vrai tant que d'autres peuvent arriver.
    fn poll(&mut self, ctx: &egui::Context, name: &str) -> bool {
        let Some(st) = &mut self.strip else { return false };
        let (new, more) = st.poll();
        for (i, img) in new {
            if let Some(s) = self.tex.get_mut(i) { *s = Some(tex_of(ctx, &format!("{name}_d{i}"), &img)); }
        }
        if !more { self.strip = None; }
        more
    }
}

/// Reglages d'un affichage.
pub struct TimelineCfg<'a> {
    /// aide affichee a droite de l'entete (molette, souris...)
    pub hint: &'a str,
    /// place laissee sous la bande de vignettes pour les etiquettes des reperes de l'appelant
    pub below: f32,
    /// la vue suit la tete de lecture (lecture en cours)
    pub follow: bool,
    /// cadence de la video : l'instant survole s'affiche en hh:mm:ss:ii ; sans cadence, en hh:mm:ss.cc
    pub fps: Option<f64>,
}

/// Geometrie de la barre affichee : de quoi passer d'un instant a une abscisse et inversement.
#[derive(Clone, Copy)]
pub struct Geo {
    pub rect: Rect,
    /// bande des vignettes
    pub strip: Rect,
    t0: f64,
    span: f64,
    dur: f64,
}

impl Geo {
    pub fn x_of(&self, t: f64) -> f32 { self.rect.min.x + ((t - self.t0) / self.span * self.rect.width() as f64) as f32 }
    pub fn t_at(&self, x: f32) -> f64 { (self.t0 + (x - self.rect.min.x) as f64 / self.rect.width() as f64 * self.span).clamp(0.0, self.dur) }
    /// Duree visible a l'ecran (secondes).
    pub fn span(&self) -> f64 { self.span }
    /// Instant du bord gauche de la vue.
    pub fn start(&self) -> f64 { self.t0 }
}

/// Pas des graduations : le plus petit qui laisse au moins 90 px entre deux graduations.
fn tick_step(span: f64, width: f32) -> f64 {
    const STEPS: [f64; 14] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0, 7200.0];
    STEPS.iter().copied().find(|s| width as f64 / (span / s) >= 90.0).unwrap_or(7200.0)
}

impl Timeline {
    pub fn new(n: usize, name: &'static str) -> Timeline {
        Timeline { zoom: 1.0, t0: 0.0, strip: None, tex: Vec::new(), n: n.max(1), name, file: None, view: (16, 9), detail: None, loading: None, want: None }
    }

    /// Lance le calcul des vignettes (`view` : taille des images affichees, pour les proportions).
    pub fn start(&mut self, file: &Path, duration: f64, view: (u32, u32)) {
        // meme taille partout (et dans le prechargement) : la frise est reprise du cache disque
        self.strip = Some(Strip::start(file, duration, self.n, strip_thumb_size(view)));
        self.tex = vec![None; self.n];
        self.file = Some(file.to_path_buf());
        self.view = view;
        self.detail = None;
        self.loading = None;
        self.want = None;
    }

    /// Vignettes de la vue zoomee : quand les cases de la frise representent bien moins de temps que les vignettes de toute la
    /// video, de nouvelles vignettes sont calculees pour la plage visible (et une demi-vue de chaque cote), des que la vue est
    /// restee stable un instant ; en dessous de 10 secondes par case, ce sont des images exactes.
    fn update_detail(&mut self, ctx: &egui::Context, dur: f64, span: f64, cell_dt: f64) {
        let mut busy = false;
        if let Some(l) = &mut self.loading {
            busy = l.poll(ctx, self.name);
            if !busy { self.detail = self.loading.take(); }
        }
        if let Some(d) = &mut self.detail { d.poll(ctx, self.name); }
        let t0 = self.t0;
        let Some(file) = self.file.clone() else { return };
        if cell_dt >= 0.8 * dur / self.n as f64 {
            // pas de zoom notable : les vignettes de toute la video suffisent
            self.detail = None;
            self.loading = None;
            self.want = None;
            return;
        }
        let loading_fits = self.loading.as_ref().is_some_and(|d| d.fits(t0, span, cell_dt));
        let detail_fits = self.detail.as_ref().is_some_and(|d| d.fits(t0, span, cell_dt));
        if loading_fits || detail_fits {
            self.want = None;
            if detail_fits && !loading_fits { self.loading = None; }
            if busy { ctx.request_repaint_after(Duration::from_millis(100)); }
            return;
        }
        // la vue change encore : on attend qu'elle se stabilise
        let key = (t0, span);
        let since = match self.want {
            Some((k, since)) if k == key => since,
            _ => { self.want = Some((key, Instant::now())); Instant::now() }
        };
        if since.elapsed() < Duration::from_millis(250) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.want = None;
        let (a, b) = ((t0 - span * 0.5).max(0.0), (t0 + span * 1.5).min(dur));
        let n = (((b - a) / cell_dt).ceil() as usize).clamp(1, 80);
        let size = (THUMB_W, scaled_height(self.view, THUMB_W));
        // images cles : une seule passe de ffmpeg ; images exactes : plusieurs ffmpeg en parallele, la vue visible d'abord
        let strip = Strip::start_range_focus(&file, a, b, n, size, ((b - a) / n as f64) < 10.0, Some(t0 + span * 0.5));
        self.loading = Some(Detail { a, b, n, strip: Some(strip), tex: vec![None; n] });
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    /// Vignette qui represente l'instant `t` : la plus fine disponible (vue zoomee en cours de calcul, puis en service), sinon
    /// celle de la video entiere.
    fn tex_at(&self, t: f64, dur: f64) -> Option<&egui::TextureHandle> {
        for d in [&self.loading, &self.detail].into_iter().flatten() {
            if let Some(Some(tx)) = d.index(t).and_then(|i| d.tex.get(i)) { return Some(tx); }
        }
        self.tex.get(self.index_at(t, dur)).and_then(|s| s.as_ref())
    }

    /// Recupere les vignettes arrivees ; vrai tant que d'autres peuvent arriver.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(st) = &mut self.strip else { return false };
        let (new, more) = st.poll();
        for (i, img) in new {
            if let Some(s) = self.tex.get_mut(i) { *s = Some(tex_of(ctx, &format!("{}_{i}", self.name), &img)); }
        }
        more
    }

    /// Dessine la barre pour une video de `dur` secondes dont la tete de lecture est a `head_t`. `overlay` dessine les reperes de
    /// l'appelant par-dessus les vignettes. Rend la reponse de la souris et la geometrie (rien si la duree est inconnue).
    pub fn show(&mut self, ui: &mut egui::Ui, dur: f64, head_t: f64, cfg: &TimelineCfg, overlay: impl FnOnce(&egui::Painter, &Geo)) -> Option<(egui::Response, Geo)> {
        ui.horizontal(|ui| {
            ui.label(tr("Timeline"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(cfg.hint).small().weak());
                if ui.small_button("+").clicked() { self.zoom = (self.zoom * 2.0).min(512.0); }
                let mut z = self.zoom.log2() as f32;
                if ui.add(egui::Slider::new(&mut z, 0.0..=9.0).show_value(false)).changed() { self.zoom = 2f64.powf(z as f64); }
                if ui.small_button("−").clicked() { self.zoom = (self.zoom / 2.0).max(1.0); }
                ui.label(tr("Zoom"));
            });
        });
        if dur <= 0.0 { return None; }
        let w = ui.available_width().max(200.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, TOP + THUMB_H + cfg.below), egui::Sense::click_and_drag());
        let mut span = dur / self.zoom;
        // la tete de lecture reste visible pendant la lecture
        if cfg.follow && (head_t < self.t0 || head_t > self.t0 + span) { self.t0 = head_t - span * 0.1; }
        self.t0 = self.t0.clamp(0.0, (dur - span).max(0.0));
        let geo_at = |t0: f64, span: f64| Geo { rect, strip: Rect::from_min_size(egui::pos2(rect.min.x, rect.min.y + TOP), egui::vec2(w, THUMB_H)), t0, span, dur };
        // molette : zoom autour du pointeur ; Maj : defilement
        if resp.hovered() {
            let (dy, dx, shift) = ui.input(|i| (i.smooth_scroll_delta.y, i.smooth_scroll_delta.x, i.modifiers.shift));
            if shift || dx.abs() > dy.abs() {
                let d = if dx.abs() > dy.abs() { dx } else { dy };
                self.t0 = (self.t0 - d as f64 / w as f64 * span).clamp(0.0, (dur - span).max(0.0));
            } else if dy != 0.0 {
                let at = ui.input(|i| i.pointer.hover_pos()).map_or(head_t, |p| geo_at(self.t0, span).t_at(p.x));
                let nz = (self.zoom * (1.0 + dy as f64 / 200.0).clamp(0.5, 2.0)).clamp(1.0, 512.0);
                let nspan = dur / nz;
                let frac = (at - self.t0) / span;
                self.zoom = nz;
                self.t0 = (at - frac * nspan).clamp(0.0, (dur - nspan).max(0.0));
                span = nspan;
            }
        }
        let geo = geo_at(self.t0, span);
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, Color32::from_gray(30));
        // graduations
        let step = tick_step(span, w);
        let mut t = (self.t0 / step).floor() * step;
        while t <= self.t0 + span {
            let x = geo.x_of(t);
            p.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + 14.0)], Stroke::new(1.0_f32, Color32::from_gray(90)));
            let s = t.round() as i64;
            let label = if step < 60.0 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 3600, s / 60 % 60) };
            p.text(egui::pos2(x + 3.0, rect.min.y), egui::Align2::LEFT_TOP, label, egui::FontId::monospace(11.0), Color32::from_gray(160));
            t += step;
        }
        // vignettes : cases de taille fixe, chacune montre la vignette la plus proche de son instant (a l'echelle de la vue)
        let cell_w = THUMB_H * 16.0 / 9.0;
        self.update_detail(ui.ctx(), dur, span, span * cell_w as f64 / w as f64);
        let mut x = geo.strip.min.x;
        while x < geo.strip.max.x {
            let cell = Rect::from_min_size(egui::pos2(x, geo.strip.min.y), egui::vec2(cell_w - 2.0, THUMB_H));
            match self.tex_at(geo.t_at(x + cell_w / 2.0), dur) {
                Some(tex) => { p.image(tex.id(), cell, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE); }
                None => { p.rect_filled(cell, 0.0, Color32::from_gray(45)); }
            }
            x += cell_w;
        }
        overlay(&p, &geo);
        // tete de lecture
        let hx = geo.x_of(head_t);
        p.line_segment([egui::pos2(hx, rect.min.y + 8.0), egui::pos2(hx, rect.max.y - 4.0)], Stroke::new(2.0_f32, HEAD));
        p.rect_filled(Rect::from_center_size(egui::pos2(hx, rect.min.y + 6.0), egui::vec2(12.0, 8.0)), 1.0, HEAD);
        // apercu au survol : la vignette la plus proche, en grand, avec son instant
        if !resp.dragged() {
            if let Some(q) = resp.hover_pos() {
                let t = geo.t_at(q.x);
                p.line_segment([egui::pos2(q.x, geo.strip.min.y), egui::pos2(q.x, geo.strip.max.y)], Stroke::new(1.0_f32, Color32::from_white_alpha(160)));
                let tex = self.tex_at(t, dur).cloned();
                let _ = resp.clone().on_hover_ui_at_pointer(|ui| {
                    if let Some(tx) = &tex {
                        let sz = tx.size_vec2();
                        ui.image((tx.id(), egui::vec2(240.0, 240.0 * sz.y / sz.x.max(1.0))));
                    }
                    ui.label(egui::RichText::new(match cfg.fps { Some(f) => bdauthor_core::timecode::format(t, f), None => tc(t) }).monospace());
                });
            }
        }
        // barre de defilement de la vue zoomee
        if self.zoom > 1.0 {
            let mut f = (self.t0 / (dur - span).max(1e-9)) as f32;
            if ui.add(egui::Slider::new(&mut f, 0.0..=1.0).show_value(false)).changed() { self.t0 = f as f64 * (dur - span).max(0.0); }
        }
        Some((resp, geo))
    }

    /// Vignette qui represente l'instant `t`.
    fn index_at(&self, t: f64, dur: f64) -> usize { ((t / dur * self.n as f64) as usize).min(self.n - 1) }
}
