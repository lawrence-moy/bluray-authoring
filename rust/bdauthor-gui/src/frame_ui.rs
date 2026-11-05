//! Selecteur d'image du widget « Image de la video » : image exacte a l'instant choisi, barre de temps avec les vignettes de la
//! video (apercu au survol), pas image par image, et recadrage a la souris. Les reglages ne sont appliques qu'a « Valider ».
use crate::chapters_ui::{tc, tex_of};
use crate::i18n::{tr, trf};
use crate::timeline_ui::{Timeline, TimelineCfg};
use bdauthor_core::design::{Kind, VideoFrame};
use bdauthor_core::editor::{crop_drag, crop_full};
use bdauthor_core::ffmpeg::{FrameStream, Tools};
use bdauthor_core::framesrc::{probe_async, Fit, FrameLoader, VideoMeta};
use bdauthor_core::model::ProjectModel;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

const VIEW_W: u32 = 960;
const STRIP_N: usize = 40;

/// Rapport impose au cadre de recadrage.
#[derive(Clone, Copy, PartialEq)]
enum Ratio { Free, Widget, R16x9, R4x3, Square }

pub struct FramePicker {
    page: usize,
    item: usize,
    video: usize,
    vfile: String,
    file: PathBuf,
    title: String,
    /// proportions du widget (largeur / hauteur)
    widget_ratio: f64,
    // brouillon
    frame: i64,
    crop: Option<[f64; 4]>,
    ratio: Ratio,
    // video
    info_rx: Option<Receiver<Result<VideoMeta, String>>>,
    info: Option<VideoMeta>,
    error: Option<String>,
    start_t: f64,
    /// images exactes (extraction en arriere-plan, gardees en memoire) et image affichee
    loader: Option<FrameLoader>,
    tex: Option<egui::TextureHandle>,
    tex_frame: i64,
    tl: Timeline,
    /// lecture (sans le son) : (flux, image de depart, images recues)
    playing: Option<(FrameStream, i64, i64)>,
    /// glissement du cadre : (poignee, cadre d'origine, point de depart en fractions)
    crop_drag: Option<(u8, [f64; 4], (f64, f64))>,
}

impl FramePicker {
    /// Selecteur pour l'element `item` de la page `page` (une « Image de la video » dont la video est choisie).
    pub fn new(model: &ProjectModel, page: usize, item: usize, base: &Path) -> Option<FramePicker> {
        let it = model.designs.free.get(page)?.items.get(item)?;
        let Kind::Image { frame: Some(f), .. } = &it.kind else { return None };
        let video = f.video?;
        let v = model.videos.get(video)?;
        let file = if Path::new(&v.file).is_absolute() { PathBuf::from(&v.file) } else { base.join(&v.file) };
        let title = bdauthor_core::editor::video_title(model, video);
        let rx = probe_async(&file, VIEW_W);
        Some(FramePicker {
            page, item, video, vfile: v.file.clone(), file, title, widget_ratio: it.w.max(1) as f64 / it.h.max(1) as f64,
            frame: 0, crop: f.crop, ratio: if f.crop.is_some() { Ratio::Free } else { Ratio::Widget },
            info_rx: Some(rx), info: None, error: None, start_t: f.at, loader: None, tex: None, tex_frame: -1,
            tl: Timeline::new(STRIP_N, "barre_image"), playing: None, crop_drag: None,
        })
    }

    fn fps(&self) -> f64 { self.info.as_ref().map_or(25.0, |i| i.fps) }
    fn duration(&self) -> f64 { self.info.as_ref().map_or(0.0, |i| i.duration) }
    fn last_frame(&self) -> i64 { ((self.duration() * self.fps()).floor() as i64 - 1).max(0) }
    fn t_of(&self, n: i64) -> f64 { n as f64 / self.fps() }
    fn seek(&mut self, n: i64) {
        self.playing = None;
        self.frame = n.clamp(0, self.last_frame());
    }

    /// Lecture / pause : le flux demarre a l'image suivante (ou au debut si l'on est a la fin).
    fn toggle_play(&mut self) {
        if self.playing.is_some() { self.playing = None; return; }
        let Some(info) = &self.info else { return };
        let size = info.size;
        if let Ok(tools) = Tools::locate() {
            let start = if self.frame >= self.last_frame() { 0 } else { self.frame + 1 };
            if let Ok(fs) = FrameStream::start(&tools, &self.file, self.t_of(start), size) {
                self.playing = Some((fs, start, 0));
            }
        }
    }

    /// Rapport impose en fractions de l'image (largeur / hauteur des fractions).
    fn ratio_frac(&self) -> Option<f64> {
        let (w, h) = self.info.as_ref().map_or((16.0, 9.0), |i| (i.size.0 as f64, i.size.1 as f64));
        let px = match self.ratio { Ratio::Free => return None, Ratio::Widget => self.widget_ratio, Ratio::R16x9 => 16.0 / 9.0, Ratio::R4x3 => 4.0 / 3.0, Ratio::Square => 1.0 };
        Some(px * h / w)
    }

    fn start_workers(&mut self) {
        let Some(info) = &self.info else { return };
        let (size, fps, dur) = (info.size, info.fps, info.duration);
        self.frame = ((self.start_t * fps).round() as i64).clamp(0, ((dur * fps).floor() as i64 - 1).max(0));
        self.loader = Some(FrameLoader::start(&self.file, Fit::Stretch, 30));
        self.tl.start(&self.file, dur, size);
    }

    /// Rend vrai s'il faut redessiner bientot.
    fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut busy = false;
        if let Some(rx) = &self.info_rx {
            match rx.try_recv() {
                Ok(Ok(i)) => { self.info = Some(i); self.info_rx = None; self.start_workers(); }
                Ok(Err(e)) => { self.error = Some(e); self.info_rx = None; }
                Err(_) => busy = true,
            }
        }
        busy |= self.tl.poll(ctx);
        // lecture : la derniere image recue est affichee
        let mut ended = false;
        if let Some((fs, start, got)) = &mut self.playing {
            let mut last = None;
            loop {
                match fs.rx.try_recv() {
                    Ok(img) => { *got += 1; last = Some(img); }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(_) => { ended = true; break; }
                }
            }
            if let Some(img) = last {
                let n = *start + *got - 1;
                self.frame = n;
                match &mut self.tex {
                    Some(t) => t.set(egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data), egui::TextureOptions::LINEAR),
                    None => self.tex = Some(tex_of(ctx, "image_video", &img)),
                }
                self.tex_frame = n;
            }
            busy = true;
        }
        if ended { self.playing = None; }
        if let (Some(info), Some(l)) = (&self.info, &mut self.loader) {
            l.poll();
            if self.playing.is_none() && self.tex_frame != self.frame {
                // image courante : en memoire, sinon demandee (juste avant l'image n : la recherche exacte ecarte les precedentes)
                l.want(self.frame, (self.frame as f64 - 0.25).max(0.0) / info.fps, info.size);
                if let Some(img) = l.get(self.frame, info.size) {
                    match &mut self.tex {
                        Some(t) => t.set(egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data), egui::TextureOptions::LINEAR),
                        None => self.tex = Some(tex_of(ctx, "image_video", &img)),
                    }
                    self.tex_frame = self.frame;
                }
            }
            busy |= l.busy();
        }
        busy
    }
}

/// Fenetre du selecteur. Rend (le projet a change, la fenetre reste ouverte).
pub fn window(ctx: &egui::Context, fp: &mut FramePicker, model: &mut ProjectModel) -> (bool, bool) {
    if fp.poll(ctx) { ctx.request_repaint_after(std::time::Duration::from_millis(20)); }
    let mut open = true;
    let mut action: Option<bool> = None;
    let title = trf!("Video image — {}", fp.title);
    crate::os_window(ctx, "selecteur_image_video", &title, [1100.0, 860.0], &mut open, &mut |ui| picker_ui(ui, fp, &mut action));
    let mut changed = false;
    if action == Some(true) {
        let t = (fp.t_of(fp.frame) * 1e4).round() / 1e4;
        let ok_video = model.videos.get(fp.video).is_some_and(|v| v.file == fp.vfile);
        if let Some(Kind::Image { frame: Some(f), .. }) = model.designs.free.get_mut(fp.page).and_then(|d| d.items.get_mut(fp.item)).map(|it| &mut it.kind) {
            if ok_video && f.video == Some(fp.video) {
                *f = VideoFrame { video: Some(fp.video), at: t, crop: fp.crop };
                changed = true;
            }
        }
    }
    (changed, open && action.is_none())
}

fn btn(ui: &mut egui::Ui, text: &str) -> bool { ui.add(egui::Button::new(text).min_size(egui::vec2(44.0, 26.0))).clicked() }

fn picker_ui(ui: &mut egui::Ui, fp: &mut FramePicker, action: &mut Option<bool>) {
    if let Some(e) = &fp.error {
        ui.colored_label(egui::Color32::from_rgb(230, 100, 90), trf!("Cannot read the video: {}", e));
        if ui.button(tr("Close")).clicked() { *action = Some(false); }
        return;
    }
    let Some(size) = fp.info.as_ref().map(|i| i.size) else {
        ui.horizontal(|ui| { ui.spinner(); ui.label(tr("Reading the video…")); });
        return;
    };
    // raccourcis : fleches (Maj 1 s, Ctrl 10 s)
    if !ui.ctx().wants_keyboard_input() {
        let (l, r, m) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.modifiers));
        let step = if m.command { (fp.fps() * 10.0).round() as i64 } else if m.shift { fp.fps().round() as i64 } else { 1 };
        if l { fp.seek(fp.frame - step); }
        if r { fp.seek(fp.frame + step); }
        if ui.input(|i| i.key_pressed(egui::Key::Space)) { fp.toggle_play(); }
    }
    // image
    let avail = ui.available_size();
    let mut iw = avail.x.max(320.0);
    let mut ih = iw * size.1 as f32 / size.0 as f32;
    let max_h = (avail.y - 250.0).max(200.0);
    if ih > max_h { ih = max_h; iw = ih * size.0 as f32 / size.1 as f32; }
    let (outer, resp) = ui.allocate_exact_size(egui::vec2(avail.x.max(iw), ih), egui::Sense::click_and_drag());
    let ir = egui::Rect::from_center_size(outer.center(), egui::vec2(iw, ih));
    let p = ui.painter_at(outer);
    p.rect_filled(ir, 0.0, egui::Color32::from_rgb(11, 15, 20));
    if let Some(t) = &fp.tex {
        p.image(t.id(), ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
    if fp.tex_frame != fp.frame {
        p.text(ir.right_bottom() - egui::vec2(8.0, 8.0), egui::Align2::RIGHT_BOTTOM, tr("decoding…"), egui::FontId::proportional(12.0), egui::Color32::from_gray(210));
    }
    // recadrage : zone gardee en clair, le reste assombri ; poignees aux angles et aux cotes
    let to_s = |x: f64, y: f64| egui::pos2(ir.min.x + x as f32 * iw, ir.min.y + y as f32 * ih);
    let to_f = |q: egui::Pos2| (((q.x - ir.min.x) / iw) as f64, ((q.y - ir.min.y) / ih) as f64);
    if let Some(c) = fp.crop {
        let cr = egui::Rect::from_min_max(to_s(c[0], c[1]), to_s(c[0] + c[2], c[1] + c[3]));
        let dim = egui::Color32::from_black_alpha(150);
        p.rect_filled(egui::Rect::from_min_max(ir.min, egui::pos2(ir.max.x, cr.min.y)), 0.0, dim);
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(ir.min.x, cr.max.y), ir.max), 0.0, dim);
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(ir.min.x, cr.min.y), egui::pos2(cr.min.x, cr.max.y)), 0.0, dim);
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(cr.max.x, cr.min.y), egui::pos2(ir.max.x, cr.max.y)), 0.0, dim);
        p.rect_stroke(cr, 0.0, egui::Stroke::new(2.0_f32, egui::Color32::WHITE));
        for q in [cr.left_top(), cr.right_top(), cr.left_bottom(), cr.right_bottom()] { p.rect_filled(egui::Rect::from_center_size(q, egui::vec2(10.0, 10.0)), 1.0, egui::Color32::WHITE); }
        for q in [cr.left_center(), cr.right_center(), cr.center_top(), cr.center_bottom()] { p.rect_filled(egui::Rect::from_center_size(q, egui::vec2(8.0, 8.0)), 1.0, egui::Color32::from_gray(220)); }
        // souris
        let handle_at = |q: egui::Pos2| -> u8 {
            let d = |a: egui::Pos2| (a - q).length() <= 9.0;
            if d(cr.left_top()) { 1 } else if d(cr.right_top()) { 2 } else if d(cr.left_bottom()) { 3 } else if d(cr.right_bottom()) { 4 }
            else if (q.x - cr.min.x).abs() <= 6.0 && q.y > cr.min.y && q.y < cr.max.y { 5 }
            else if (q.x - cr.max.x).abs() <= 6.0 && q.y > cr.min.y && q.y < cr.max.y { 6 }
            else if (q.y - cr.min.y).abs() <= 6.0 && q.x > cr.min.x && q.x < cr.max.x { 7 }
            else if (q.y - cr.max.y).abs() <= 6.0 && q.x > cr.min.x && q.x < cr.max.x { 8 }
            else if cr.contains(q) { 0 } else { 9 }
        };
        if let Some(q) = resp.hover_pos() {
            let icon = match handle_at(q) { 1 | 4 => egui::CursorIcon::ResizeNwSe, 2 | 3 => egui::CursorIcon::ResizeNeSw, 5 | 6 => egui::CursorIcon::ResizeHorizontal, 7 | 8 => egui::CursorIcon::ResizeVertical, 0 => egui::CursorIcon::Grab, _ => egui::CursorIcon::Crosshair };
            ui.ctx().set_cursor_icon(icon);
        }
        if resp.drag_started() {
            if let Some(q) = resp.interact_pointer_pos() { fp.crop_drag = Some((handle_at(q), c, to_f(q))); }
        }
        if resp.dragged() {
            if let (Some((h, o, s)), Some(q)) = (fp.crop_drag, resp.interact_pointer_pos()) {
                fp.crop = Some(crop_drag(h, o, s, to_f(q), fp.ratio_frac()));
            }
        }
        if resp.drag_stopped() { fp.crop_drag = None; }
    }
    ui.add_space(6.0);
    // pas image par image
    ui.horizontal(|ui| {
        let s1 = fp.fps().round() as i64;
        if btn(ui, "|◀") { fp.seek(0); }
        if btn(ui, tr("−10 s")) { fp.seek(fp.frame - 10 * s1); }
        if btn(ui, tr("−1 s")) { fp.seek(fp.frame - s1); }
        if btn(ui, tr("◀ frame")) { fp.seek(fp.frame - 1); }
        let play = if fp.playing.is_some() { tr("■ Pause") } else { tr("▶ Play") };
        if ui.add(egui::Button::new(play).min_size(egui::vec2(70.0, 26.0)).fill(egui::Color32::from_rgb(44, 93, 135))).clicked() { fp.toggle_play(); }
        if btn(ui, tr("frame ▶")) { fp.seek(fp.frame + 1); }
        if btn(ui, tr("+1 s")) { fp.seek(fp.frame + s1); }
        if btn(ui, tr("+10 s")) { fp.seek(fp.frame + 10 * s1); }
        if btn(ui, "▶|") { let n = fp.last_frame(); fp.seek(n); }
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tc(fp.t_of(fp.frame))).monospace().strong());
        ui.label(egui::RichText::new(format!("/ {}", tc(fp.duration()))).monospace().weak());
    });
    ui.add_space(4.0);
    timeline(ui, fp);
    ui.label(egui::RichText::new(tr("Click or drag on the bar to choose the time (hover: preview) · Space: play / pause · ← →: one frame · Shift: 1 s · Ctrl: 10 s")).small().weak());
    ui.separator();
    // recadrage
    ui.horizontal_wrapped(|ui| {
        let mut on = fp.crop.is_some();
        if ui.checkbox(&mut on, tr("Crop the image")).changed() {
            fp.crop = if on { Some(crop_full(fp.ratio_frac())) } else { None };
        }
        if fp.crop.is_some() {
            ui.label(tr("Proportions:"));
            let names = [(Ratio::Free, tr("free")), (Ratio::Widget, tr("those of the widget")), (Ratio::R16x9, "16:9"), (Ratio::R4x3, "4:3"), (Ratio::Square, "1:1")];
            let cur = names.iter().find(|n| n.0 == fp.ratio).map_or("", |n| n.1);
            let mut pick = None;
            egui::ComboBox::from_id_salt("rapport_recadrage").selected_text(cur).show_ui(ui, |ui| {
                for (r, n) in names { if ui.selectable_label(fp.ratio == r, n).clicked() { pick = Some(r); } }
            });
            if let Some(r) = pick {
                fp.ratio = r;
                if r != Ratio::Free { fp.crop = Some(crop_full(fp.ratio_frac())); }
            }
            if ui.button(tr("Largest frame")).on_hover_text(tr("The largest frame with these proportions, centered")).clicked() { fp.crop = Some(crop_full(fp.ratio_frac())); }
            if let Some(c) = fp.crop {
                let (w, h) = (c[2] * size.0 as f64 * 2.0, c[3] * size.1 as f64 * 2.0);
                ui.label(egui::RichText::new(trf!("kept: {} %", format!("{:.0}", c[2] * c[3] * 100.0))).weak()).on_hover_text(trf!("about {} × {} px of a 1920-wide image", format!("{:.0}", w), format!("{:.0}", h)));
            }
        } else {
            ui.label(egui::RichText::new(tr("Without cropping: the center of the image in 16:9.")).weak());
        }
    });
    if fp.crop.is_some() {
        ui.label(egui::RichText::new(tr("Drag the frame to move it, its corners or sides to resize it, or draw a new frame outside it.")).small().weak());
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button(tr("Cancel")).clicked() { *action = Some(false); }
        if ui.add(egui::Button::new(egui::RichText::new(tr("Confirm")).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(44, 93, 135))).clicked() { *action = Some(true); }
    });
}

/// Barre de temps commune (vignettes, zoom, survol) ; clic ou glisser = aller a cet instant.
fn timeline(ui: &mut egui::Ui, fp: &mut FramePicker) {
    let head_t = fp.t_of(fp.frame);
    let cfg = TimelineCfg { hint: tr("Wheel: zoom · Shift + wheel: scroll · click or drag: choose the time"), below: 4.0, follow: fp.playing.is_some() };
    let dur = fp.duration();
    let Some((resp, geo)) = fp.tl.show(ui, dur, head_t, &cfg, |_, _| {}) else { return };
    if let Some(q) = resp.interact_pointer_pos() {
        if resp.clicked() || resp.dragged() {
            let n = ((geo.t_at(q.x) * fp.fps()).round() as i64).clamp(0, fp.last_frame());
            fp.seek(n);
        }
    }
}
