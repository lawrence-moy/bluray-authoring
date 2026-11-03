//! Editeur de chapitres d'une video : image a l'instant choisi (image par image), lecture sans le son, frise avec les vignettes
//! de la video et les reperes de chapitre, choix des images de vignette. Les modifications restent dans la fenetre jusqu'a
//! « Valider ».
use crate::i18n::{tr, trf};
use bdauthor_core::design::Kind;
use bdauthor_core::ffmpeg::{extract_frame_rgba, list_keyframes, probe, FrameStream, Tools};
use bdauthor_core::model::{ChapterEntry, ProjectModel};
use bdauthor_core::palette::RgbaImage;
use eframe::egui;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Largeur des images decodees pour l'affichage (la hauteur suit les proportions de la video).
const VIEW_W: u32 = 960;
/// Vignettes de la frise (toute la video).
const STRIP_N: usize = 48;
const STRIP_W: u32 = 160;
/// Images gardees en memoire (environ 2 Mo chacune).
const CACHE_MAX: usize = 40;

const MARK: egui::Color32 = egui::Color32::from_rgb(230, 195, 74);
const HEAD: egui::Color32 = egui::Color32::from_rgb(255, 140, 58);

struct Info {
    duration: f64,
    fps: f64,
    size: (u32, u32),
}

pub struct ChapterEditor {
    pub video: usize,
    /// chemin de la video tel qu'enregistre dans le projet (pour verifier a la validation qu'il s'agit toujours d'elle)
    vfile: String,
    file: PathBuf,
    title: String,
    // brouillon (applique par « Valider »)
    chapters: Vec<ChapterEntry>,
    next_id: u32,
    thumb_s: Option<f64>,
    exact: bool,
    /// vignettes de pages liees a un chapitre : (nombre, image choisie) ; modifiees ici, appliquees a la validation
    slots: HashMap<u32, (usize, Option<f64>)>,
    slots_changed: bool,
    // video
    info_rx: Option<Receiver<Result<Info, String>>>,
    info: Option<Info>,
    error: Option<String>,
    frame: i64,
    sel: Option<u32>,
    // images
    req_tx: Option<Sender<i64>>,
    res_rx: Option<Receiver<(i64, Result<RgbaImage, String>)>>,
    pending: bool,
    requested: i64,
    cache: HashMap<i64, RgbaImage>,
    order: VecDeque<i64>,
    tex: Option<egui::TextureHandle>,
    tex_frame: i64,
    /// echec du decodage d'une image : (image, message)
    frame_error: Option<(i64, String)>,
    // frise
    strip_rx: Option<Receiver<(usize, RgbaImage)>>,
    strip: Vec<Option<egui::TextureHandle>>,
    keys_rx: Option<Receiver<Result<Vec<f64>, String>>>,
    keys: Option<Vec<f64>>,
    zoom: f64,
    view_t0: f64,
    drag_ch: Option<u32>,
    // lecture : (flux, image de depart, images recues)
    playing: Option<(FrameStream, i64, i64)>,
}

/// « h:mm:ss.cc »
fn tc(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as i64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

fn tex_of(ctx: &egui::Context, name: &str, img: &RgbaImage) -> egui::TextureHandle {
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
    ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
}

impl ChapterEditor {
    pub fn new(model: &ProjectModel, video: usize, base: &Path) -> Option<ChapterEditor> {
        let v = model.videos.get(video)?;
        let file = if Path::new(&v.file).is_absolute() { PathBuf::from(&v.file) } else { base.join(&v.file) };
        let name = Path::new(&v.file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| v.file.clone());
        let title = if v.title.is_empty() { name } else { v.title.clone() };
        let mut slots: HashMap<u32, (usize, Option<f64>)> = HashMap::new();
        for d in &model.designs.free {
            for it in &d.items {
                if let Kind::Slot { video: Some(vi), chapter: Some(id), thumb_at, .. } = &it.kind {
                    if *vi == video {
                        let e = slots.entry(*id).or_insert((0, *thumb_at));
                        e.0 += 1;
                    }
                }
            }
        }
        let (tx, rx) = channel();
        let f = file.clone();
        std::thread::spawn(move || {
            let r = Tools::locate().map_err(|e| e.to_string()).and_then(|t| probe(&t, &f).map_err(|e| e.to_string())).map(|i| {
                let (w, h) = (i.width.max(1), i.height.max(1));
                let vh = ((VIEW_W as f64 * h as f64 / w as f64 / 2.0).round() as u32 * 2).max(2);
                Info { duration: i.duration_s, fps: if i.fps() > 1.0 { i.fps() } else { 25.0 }, size: (VIEW_W, vh) }
            });
            let _ = tx.send(r);
        });
        let mut chapters = v.chapters.clone();
        chapters.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
        Some(ChapterEditor {
            video, vfile: v.file.clone(), file, title, next_id: v.next_chapter_id(), chapters, thumb_s: v.thumb_s, exact: v.exact_chapters, slots, slots_changed: false,
            info_rx: Some(rx), info: None, error: None, frame: 0, sel: None,
            req_tx: None, res_rx: None, pending: false, requested: -1, cache: HashMap::new(), order: VecDeque::new(), tex: None, tex_frame: -1, frame_error: None,
            strip_rx: None, strip: Vec::new(), keys_rx: None, keys: None, zoom: 1.0, view_t0: 0.0, drag_ch: None, playing: None,
        })
    }

    fn fps(&self) -> f64 { self.info.as_ref().map_or(25.0, |i| i.fps) }
    fn duration(&self) -> f64 { self.info.as_ref().map_or(0.0, |i| i.duration) }
    fn last_frame(&self) -> i64 { ((self.duration() * self.fps()).floor() as i64 - 1).max(0) }
    fn t_of(&self, n: i64) -> f64 { n as f64 / self.fps() }
    /// Instant enregistre pour l'image `n` (au dix-millieme de seconde).
    fn stored_t(&self, n: i64) -> f64 { (self.t_of(n) * 1e4).round() / 1e4 }
    fn frame_of(&self, t: f64) -> i64 { ((t * self.fps()).round() as i64).clamp(0, self.last_frame()) }

    fn seek(&mut self, n: i64) {
        self.playing = None;
        self.frame = n.clamp(0, self.last_frame());
    }

    /// Lance les fils de calcul une fois la video sondee.
    fn start_workers(&mut self) {
        let Some(info) = &self.info else { return };
        let (size, fps, dur) = (info.size, info.fps, info.duration);
        // images exactes : une seule demande a la fois, la plus recente
        let (req_tx, req_rx) = channel::<i64>();
        let (res_tx, res_rx) = channel();
        let src = self.file.clone();
        std::thread::spawn(move || {
            let tools = Tools::locate();
            while let Ok(mut n) = req_rx.recv() {
                while let Ok(m) = req_rx.try_recv() { n = m; }
                // juste avant l'image n : la recherche exacte ecarte les images anterieures
                let t = (n as f64 - 0.25).max(0.0) / fps;
                let r = match &tools {
                    Ok(tl) => extract_frame_rgba(tl, &src, t, size, true).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                if res_tx.send((n, r)).is_err() { break; }
            }
        });
        self.req_tx = Some(req_tx);
        self.res_rx = Some(res_rx);
        // vignettes de la frise : images cles (rapides), du debut a la fin
        let (stx, srx) = channel();
        let src = self.file.clone();
        let sh = ((STRIP_W as f64 * size.1 as f64 / size.0 as f64 / 2.0).round() as u32 * 2).max(2);
        std::thread::spawn(move || {
            let Ok(tools) = Tools::locate() else { return };
            for i in 0..STRIP_N {
                let t = (i as f64 + 0.5) * dur / STRIP_N as f64;
                if let Ok(img) = extract_frame_rgba(&tools, &src, t, (STRIP_W, sh), false) {
                    if stx.send((i, img)).is_err() { return; }
                }
            }
        });
        self.strip_rx = Some(srx);
        self.strip = vec![None; STRIP_N];
        let (ktx, krx) = channel();
        let src = self.file.clone();
        std::thread::spawn(move || {
            let r = Tools::locate().map_err(|e| e.to_string()).and_then(|t| list_keyframes(&t, &src).map_err(|e| e.to_string()));
            let _ = ktx.send(r);
        });
        self.keys_rx = Some(krx);
    }

    /// Resultats des fils de calcul ; rend vrai s'il faut redessiner bientot.
    fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut busy = false;
        if let Some(rx) = &self.info_rx {
            match rx.try_recv() {
                Ok(Ok(i)) => { self.info = Some(i); self.info_rx = None; self.start_workers(); }
                Ok(Err(e)) => { self.error = Some(e); self.info_rx = None; }
                Err(_) => busy = true,
            }
        }
        if let Some(rx) = &self.res_rx {
            while let Ok((n, r)) = rx.try_recv() {
                match r {
                    Ok(img) => {
                        self.cache.insert(n, img);
                        self.order.push_back(n);
                        while self.order.len() > CACHE_MAX { if let Some(o) = self.order.pop_front() { self.cache.remove(&o); } }
                        self.frame_error = None;
                    }
                    Err(e) => self.frame_error = Some((n, e)),
                }
                if n == self.requested { self.pending = false; }
            }
        }
        if let Some(rx) = &self.strip_rx {
            loop {
                match rx.try_recv() {
                    Ok((i, img)) => { if let Some(s) = self.strip.get_mut(i) { *s = Some(tex_of(ctx, &format!("frise_{i}"), &img)); } }
                    Err(std::sync::mpsc::TryRecvError::Empty) => { busy = true; break; }
                    Err(_) => { self.strip_rx = None; break; }
                }
            }
        }
        if let Some(rx) = &self.keys_rx {
            match rx.try_recv() {
                Ok(r) => { self.keys = r.ok(); self.keys_rx = None; }
                Err(_) => busy = true,
            }
        }
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
                    None => self.tex = Some(tex_of(ctx, "chapitres_image", &img)),
                }
                self.tex_frame = n;
            }
            busy = true;
        }
        if ended { self.playing = None; }
        // image courante : en memoire, sinon demandee
        if self.playing.is_none() && self.info.is_some() && self.tex_frame != self.frame {
            if let Some(img) = self.cache.get(&self.frame) {
                match &mut self.tex {
                    Some(t) => t.set(egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data), egui::TextureOptions::LINEAR),
                    None => self.tex = Some(tex_of(ctx, "chapitres_image", img)),
                }
                self.tex_frame = self.frame;
            } else if let Some(tx) = &self.req_tx {
                // une demande par image (le fil de calcul ne traite que la plus recente)
                if self.requested != self.frame {
                    let _ = tx.send(self.frame);
                    self.requested = self.frame;
                    self.pending = true;
                }
            }
        }
        busy || self.pending
    }

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

    fn chapter_at(&self, n: i64) -> Option<&ChapterEntry> {
        let t = self.t_of(n) + 1e-6;
        self.chapters.iter().filter(|c| c.t <= t).last()
    }

    fn add_here(&mut self) {
        let t = self.stored_t(self.frame);
        if let Some(c) = self.chapters.iter().find(|c| (c.t - t).abs() < 0.5 / self.fps()) { self.sel = Some(c.id); return; }
        let id = self.next_id;
        self.next_id += 1;
        self.chapters.push(ChapterEntry { id, t, title: String::new() });
        self.sort();
        self.sel = Some(id);
    }

    fn sort(&mut self) { self.chapters.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal)); }

    /// Raccourcis clavier (sauf pendant la saisie d'un nom).
    fn keys_input(&mut self, ui: &egui::Ui) {
        if ui.ctx().wants_keyboard_input() { return; }
        let (l, r, space, c, home, end, m) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Space),
            i.key_pressed(egui::Key::C), i.key_pressed(egui::Key::Home), i.key_pressed(egui::Key::End), i.modifiers));
        let step = if m.command { (self.fps() * 10.0).round() as i64 } else if m.shift { self.fps().round() as i64 } else { 1 };
        if l { self.seek(self.frame - step); }
        if r { self.seek(self.frame + step); }
        if home { self.seek(0); }
        if end { let n = self.last_frame(); self.seek(n); }
        if space { self.toggle_play(); }
        if c { self.add_here(); }
    }

    /// Image cle qui precede (`dir` < 0) ou qui suit l'image courante.
    fn key_jump(&self, dir: i32) -> Option<i64> {
        let k = self.keys.as_ref()?;
        let t = self.t_of(self.frame);
        let eps = 0.5 / self.fps();
        let x = if dir < 0 { k.iter().rev().find(|&&x| x < t - eps) } else { k.iter().find(|&&x| x > t + eps) };
        x.map(|&x| self.frame_of(x))
    }
}

/// Fenetre de l'editeur. Rend (le projet a change, la fenetre reste ouverte).
pub fn window(ctx: &egui::Context, ed: &mut ChapterEditor, model: &mut ProjectModel) -> (bool, bool) {
    if ed.poll(ctx) { ctx.request_repaint_after(std::time::Duration::from_millis(15)); }
    let mut open = true;
    let mut action: Option<bool> = None;
    let title = trf!("Chapter editor — {}", ed.title);
    crate::os_window_panels(ctx, "editeur_chapitres", &title, [1400.0, 880.0], &mut open, Some(210.0), &mut |ui, bottom| {
        if bottom { timeline(ui, ed); } else { main_area(ui, ed, &mut action); }
    });
    let mut changed = false;
    if action == Some(true) {
        if let Some(v) = model.videos.get_mut(ed.video).filter(|v| v.file == ed.vfile) {
            ed.sort();
            v.chapters = ed.chapters.clone();
            v.thumb_s = ed.thumb_s;
            v.exact_chapters = ed.exact;
        }
        if ed.slots_changed {
            for d in &mut model.designs.free {
                for it in &mut d.items {
                    if let Kind::Slot { video: Some(vi), chapter: Some(id), thumb_at, .. } = &mut it.kind {
                        if *vi == ed.video {
                            if let Some((_, t)) = ed.slots.get(id) { *thumb_at = *t; }
                        }
                    }
                }
            }
        }
        model.sync_chapters();
        changed = true;
    }
    (changed, open && action.is_none())
}

fn small_btn(ui: &mut egui::Ui, text: &str) -> bool { ui.add(egui::Button::new(text).min_size(egui::vec2(46.0, 28.0))).clicked() }

fn main_area(ui: &mut egui::Ui, ed: &mut ChapterEditor, action: &mut Option<bool>) {
    if let Some(e) = &ed.error {
        ui.colored_label(egui::Color32::from_rgb(230, 100, 90), trf!("Cannot read the video: {}", e));
        if ui.button(tr("Close")).clicked() { *action = Some(false); }
        return;
    }
    if ed.info.is_none() {
        ui.horizontal(|ui| { ui.spinner(); ui.label(tr("Reading the video…")); });
        return;
    }
    ed.keys_input(ui);
    let size = ed.info.as_ref().map_or((VIEW_W, 540), |i| i.size);
    let panel_w = 390.0;
    let avail = ui.available_size();
    let mut img_w = (avail.x - panel_w - 20.0).max(320.0);
    let mut img_h = img_w * size.1 as f32 / size.0 as f32;
    let max_h = (avail.y - 90.0).max(180.0);
    if img_h > max_h { img_h = max_h; img_w = img_h * size.0 as f32 / size.1 as f32; }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(img_w);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(img_w, img_h), egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 0.0, egui::Color32::from_rgb(11, 15, 20));
            if let Some(t) = &ed.tex {
                p.image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            let info_txt = trf!("{} · frame {}", tc(ed.t_of(ed.frame)), ed.frame);
            let g = p.layout_no_wrap(info_txt, egui::FontId::monospace(13.0), egui::Color32::from_gray(240));
            let box_r = egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), g.size() + egui::vec2(12.0, 6.0));
            p.rect_filled(box_r, 2.0, egui::Color32::from_black_alpha(160));
            p.galley(box_r.min + egui::vec2(6.0, 3.0), g, egui::Color32::from_gray(240));
            if let Some(c) = ed.chapter_at(ed.frame) {
                let n = ed.chapters.iter().position(|x| x.id == c.id).unwrap_or(0) + 1;
                let txt = if c.title.is_empty() { trf!("Chapter {}", n) } else { trf!("Chapter {} — {}", n, c.title) };
                let g = p.layout_no_wrap(txt, egui::FontId::proportional(13.0), MARK);
                let box_r = egui::Rect::from_min_size(egui::pos2(rect.max.x - g.size().x - 22.0, rect.min.y + 10.0), g.size() + egui::vec2(12.0, 6.0));
                p.rect_filled(box_r, 2.0, egui::Color32::from_black_alpha(160));
                p.galley(box_r.min + egui::vec2(6.0, 3.0), g, MARK);
            }
            if ed.tex_frame != ed.frame && ed.playing.is_none() {
                if ed.frame_error.as_ref().is_some_and(|e| e.0 == ed.frame) {
                    p.text(rect.center(), egui::Align2::CENTER_CENTER, tr("Image unavailable"), egui::FontId::proportional(16.0), egui::Color32::from_rgb(230, 100, 90));
                } else {
                    p.text(rect.right_bottom() - egui::vec2(10.0, 10.0), egui::Align2::RIGHT_BOTTOM, tr("decoding…"), egui::FontId::proportional(12.0), egui::Color32::from_gray(200));
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let f = ed.fps();
                let s1 = f.round() as i64;
                if small_btn(ui, "|◀") { ed.seek(0); }
                if small_btn(ui, tr("−10 s")) { ed.seek(ed.frame - 10 * s1); }
                if small_btn(ui, tr("−1 s")) { ed.seek(ed.frame - s1); }
                if small_btn(ui, tr("◀ frame")) { ed.seek(ed.frame - 1); }
                let play = if ed.playing.is_some() { tr("■ Pause") } else { tr("▶ Play") };
                if ui.add(egui::Button::new(play).min_size(egui::vec2(70.0, 28.0)).fill(egui::Color32::from_rgb(44, 93, 135))).clicked() { ed.toggle_play(); }
                if small_btn(ui, tr("frame ▶")) { ed.seek(ed.frame + 1); }
                if small_btn(ui, tr("+1 s")) { ed.seek(ed.frame + s1); }
                if small_btn(ui, tr("+10 s")) { ed.seek(ed.frame + 10 * s1); }
                if small_btn(ui, "▶|") { let n = ed.last_frame(); ed.seek(n); }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(tc(ed.t_of(ed.frame))).monospace().strong());
                ui.label(egui::RichText::new(format!("/ {}", tc(ed.duration()))).monospace().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(tr("+ Chapter here (C)")).color(egui::Color32::from_rgb(240, 215, 122))).fill(egui::Color32::from_rgb(74, 63, 28)).min_size(egui::vec2(0.0, 28.0))).clicked() { ed.add_here(); }
                });
            });
            ui.label(egui::RichText::new(tr("← →: one frame · Shift: 1 s · Ctrl: 10 s · Space: play / pause · C: chapter here · playback without sound")).small().weak());
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_width(panel_w);
            side_panel(ui, ed, action);
        });
    });
}

fn side_panel(ui: &mut egui::Ui, ed: &mut ChapterEditor, action: &mut Option<bool>) {
    ui.heading(tr("Chapters"));
    let mut go = None;
    let mut del = None;
    egui::ScrollArea::vertical().id_salt("liste_chapitres").max_height(260.0).show(ui, |ui| {
        if ed.chapters.is_empty() {
            ui.label(egui::RichText::new(tr("No chapter yet: place the image and press “+ Chapter here”.")).weak());
        }
        for (k, c) in ed.chapters.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let selected = ed.sel == Some(c.id);
                if ui.selectable_label(selected, egui::RichText::new(format!("{:>2}", k + 1)).color(MARK).strong()).clicked() { ed.sel = Some(c.id); go = Some(c.t); }
                if ui.link(egui::RichText::new(tc(c.t)).monospace()).on_hover_text(tr("Go to this chapter")).clicked() { ed.sel = Some(c.id); go = Some(c.t); }
                ui.add(egui::TextEdit::singleline(&mut c.title).hint_text(tr("name (optional)")).desired_width(170.0));
                if crate::delete_button(ui, tr("Remove this chapter")) { del = Some(c.id); }
            });
        }
    });
    if let Some(t) = go { let n = ed.frame_of(t); ed.seek(n); }
    if let Some(id) = del { ed.chapters.retain(|c| c.id != id); if ed.sel == Some(id) { ed.sel = None; } }
    ui.horizontal_wrapped(|ui| {
        let sel_n = ed.sel.and_then(|id| ed.chapters.iter().position(|c| c.id == id));
        if ui.add_enabled(sel_n.is_some(), egui::Button::new(match sel_n { Some(n) => trf!("Move chapter {} here", n + 1), None => tr("Move the selected chapter here").to_string() })).clicked() {
            let t = ed.stored_t(ed.frame);
            if let Some(c) = ed.sel.and_then(|id| ed.chapters.iter_mut().find(|c| c.id == id)) { c.t = t; }
            ed.sort();
        }
        let prev = ed.key_jump(-1);
        let next = ed.key_jump(1);
        let tip = if ed.keys.is_some() { tr("Key frames of the source video: where a player can start without decoding earlier images") } else { tr("Reading the key frames…") };
        if ui.add_enabled(prev.is_some(), egui::Button::new(tr("◀ Key frame"))).on_hover_text(tip).clicked() { if let Some(n) = prev { ed.seek(n); } }
        if ui.add_enabled(next.is_some(), egui::Button::new(tr("Key frame ▶"))).on_hover_text(tip).clicked() { if let Some(n) = next { ed.seek(n); } }
    });

    ui.separator();
    ui.heading(tr("Thumbnails"));
    ui.horizontal(|ui| {
        ui.label(tr("Video thumbnail:"));
        match ed.thumb_s {
            Some(t) => { if ui.link(egui::RichText::new(tc(t)).monospace()).clicked() { let n = ed.frame_of(t); ed.seek(n); } }
            None => { ui.label(egui::RichText::new(tr("automatic")).weak()); }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(tr("Use the displayed image")).clicked() { ed.thumb_s = Some(ed.stored_t(ed.frame)); }
        if ed.thumb_s.is_some() && ui.button(tr("Automatic")).clicked() { ed.thumb_s = None; }
    });
    ui.add_space(4.0);
    match ed.sel.and_then(|id| ed.chapters.iter().position(|c| c.id == id).map(|n| (id, n))) {
        None => { ui.label(egui::RichText::new(tr("Select a chapter to choose the image of its page thumbnails.")).weak()); }
        Some((id, n)) => match ed.slots.get(&id).copied() {
            None => { ui.label(egui::RichText::new(trf!("No page thumbnail is linked to chapter {}.", n + 1)).weak()); }
            Some((count, at)) => {
                ui.horizontal(|ui| {
                    ui.label(trf!("Chapter {} thumbnail ({} linked):", n + 1, count));
                    match at {
                        Some(t) => { if ui.link(egui::RichText::new(tc(t)).monospace()).clicked() { let f = ed.frame_of(t); ed.seek(f); } }
                        None => { ui.label(egui::RichText::new(tr("follows the chapter")).weak()); }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(tr("Use the displayed image")).clicked() {
                        let t = ed.stored_t(ed.frame);
                        if let Some(e) = ed.slots.get_mut(&id) { e.1 = Some(t); }
                        ed.slots_changed = true;
                    }
                    if at.is_some() && ui.button(tr("Follow the chapter")).clicked() {
                        if let Some(e) = ed.slots.get_mut(&id) { e.1 = None; }
                        ed.slots_changed = true;
                    }
                });
            }
        },
    }

    ui.separator();
    ui.checkbox(&mut ed.exact, tr("Exact key frame on each chapter")).on_hover_text(tr("The video is encoded with a key frame on each chapter: the player starts exactly on the chosen image (otherwise on the previous key frame, up to 1 s earlier). Changing a chapter then re-encodes the video."));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui.button(tr("Cancel")).clicked() { *action = Some(false); }
        if ui.add(egui::Button::new(egui::RichText::new(tr("Confirm")).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(44, 93, 135))).clicked() { *action = Some(true); }
    });
}

/// Pas des graduations : le plus petit qui laisse au moins 90 px entre deux graduations.
fn tick_step(span: f64, width: f32) -> f64 {
    const STEPS: [f64; 14] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0, 7200.0];
    STEPS.iter().copied().find(|s| width as f64 / (span / s) >= 90.0).unwrap_or(7200.0)
}

fn timeline(ui: &mut egui::Ui, ed: &mut ChapterEditor) {
    let dur = ed.duration();
    ui.horizontal(|ui| {
        ui.label(tr("Timeline"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(tr("Wheel: zoom · Shift + wheel: scroll · drag a mark: move the chapter · click: go to this time")).small().weak());
            if ui.small_button("+").clicked() { ed.zoom = (ed.zoom * 2.0).min(512.0); }
            let mut z = ed.zoom.log2() as f32;
            if ui.add(egui::Slider::new(&mut z, 0.0..=9.0).show_value(false)).changed() { ed.zoom = 2f64.powf(z as f64); }
            if ui.small_button("−").clicked() { ed.zoom = (ed.zoom / 2.0).max(1.0); }
            ui.label(tr("Zoom"));
        });
    });
    if dur <= 0.0 { return; }
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 140.0), egui::Sense::click_and_drag());
    let span = dur / ed.zoom;
    // la tete de lecture reste visible pendant la lecture
    let head_t = ed.t_of(ed.frame);
    if ed.playing.is_some() && (head_t < ed.view_t0 || head_t > ed.view_t0 + span) { ed.view_t0 = head_t - span * 0.1; }
    ed.view_t0 = ed.view_t0.clamp(0.0, (dur - span).max(0.0));
    let t0 = ed.view_t0;
    let x_of = |t: f64| rect.min.x + ((t - t0) / span * w as f64) as f32;
    let t_of_x = |x: f32| (t0 + (x - rect.min.x) as f64 / w as f64 * span).clamp(0.0, dur);
    // molette : zoom autour du pointeur ; Maj : defilement
    if resp.hovered() {
        let (dy, dx, shift) = ui.input(|i| (i.smooth_scroll_delta.y, i.smooth_scroll_delta.x, i.modifiers.shift));
        if shift || dx.abs() > dy.abs() {
            let d = if dx.abs() > dy.abs() { dx } else { dy };
            ed.view_t0 = (ed.view_t0 - d as f64 / w as f64 * span).clamp(0.0, (dur - span).max(0.0));
        } else if dy != 0.0 {
            let at = ui.input(|i| i.pointer.hover_pos()).map_or(head_t, |p| t_of_x(p.x));
            let nz = (ed.zoom * (1.0 + dy as f64 / 200.0).clamp(0.5, 2.0)).clamp(1.0, 512.0);
            let nspan = dur / nz;
            let frac = (at - t0) / span;
            ed.zoom = nz;
            ed.view_t0 = (at - frac * nspan).clamp(0.0, (dur - nspan).max(0.0));
        }
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, egui::Color32::from_gray(30));
    // graduations
    let step = tick_step(span, w);
    let mut t = (t0 / step).floor() * step;
    while t <= t0 + span {
        let x = x_of(t);
        p.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + 14.0)], egui::Stroke::new(1.0, egui::Color32::from_gray(90)));
        let s = t.round() as i64;
        let label = if step < 60.0 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 3600, s / 60 % 60) };
        p.text(egui::pos2(x + 3.0, rect.min.y), egui::Align2::LEFT_TOP, label, egui::FontId::monospace(11.0), egui::Color32::from_gray(160));
        t += step;
    }
    // vignettes : cases de taille fixe, chacune montre la vignette la plus proche de son instant
    let strip = egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.min.y + 18.0), egui::vec2(w, 64.0));
    let cell_w = 64.0 * 16.0 / 9.0;
    let mut x = strip.min.x;
    while x < strip.max.x {
        let cell = egui::Rect::from_min_size(egui::pos2(x, strip.min.y), egui::vec2(cell_w - 2.0, 64.0));
        let tc_ = t_of_x(x + cell_w / 2.0);
        let i = ((tc_ / dur * STRIP_N as f64) as usize).min(STRIP_N - 1);
        match ed.strip.get(i).and_then(|s| s.as_ref()) {
            Some(tex) => { p.image(tex.id(), cell, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE); }
            None => { p.rect_filled(cell, 0.0, egui::Color32::from_gray(45)); }
        }
        x += cell_w;
    }
    // images cles de la source, quand l'echelle permet de les distinguer
    if let Some(keys) = &ed.keys {
        if span < 600.0 {
            for &k in keys.iter().filter(|&&k| k >= t0 && k <= t0 + span) {
                let x = x_of(k);
                p.line_segment([egui::pos2(x, strip.max.y - 8.0), egui::pos2(x, strip.max.y)], egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 200, 255)));
            }
        }
    }
    // reperes de chapitre
    for (k, c) in ed.chapters.iter().enumerate() {
        if c.t < t0 - span * 0.05 || c.t > t0 + span { continue; }
        let x = x_of(c.t);
        let sel = ed.sel == Some(c.id);
        p.line_segment([egui::pos2(x, rect.min.y + 14.0), egui::pos2(x, rect.min.y + 98.0)], egui::Stroke::new(if sel { 3.0 } else { 2.0 }, MARK));
        let txt = if c.title.is_empty() { format!("{}", k + 1) } else { format!("{} · {}", k + 1, c.title) };
        let g = p.layout_no_wrap(txt, egui::FontId::proportional(11.0), egui::Color32::from_rgb(240, 215, 122));
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.min.y + 98.0), g.size() + egui::vec2(10.0, 4.0));
        p.rect_filled(r, 2.0, if sel { egui::Color32::from_rgb(110, 92, 30) } else { egui::Color32::from_rgb(74, 63, 28) });
        p.galley(r.min + egui::vec2(5.0, 2.0), g, egui::Color32::from_rgb(240, 215, 122));
    }
    // tete de lecture
    let hx = x_of(head_t);
    p.line_segment([egui::pos2(hx, rect.min.y + 8.0), egui::pos2(hx, rect.max.y - 4.0)], egui::Stroke::new(2.0, HEAD));
    p.rect_filled(egui::Rect::from_center_size(egui::pos2(hx, rect.min.y + 6.0), egui::vec2(12.0, 8.0)), 1.0, HEAD);
    // souris : glisser un repere le deplace ; ailleurs, cliquer ou glisser deplace la tete de lecture
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.drag_started() {
            ed.drag_ch = ed.chapters.iter().filter(|c| (x_of(c.t) - pos.x).abs() <= 6.0).min_by(|a, b| (x_of(a.t) - pos.x).abs().total_cmp(&(x_of(b.t) - pos.x).abs())).map(|c| c.id);
            if let Some(id) = ed.drag_ch { ed.sel = Some(id); }
        }
        let n = ed.frame_of(t_of_x(pos.x));
        match ed.drag_ch {
            Some(id) if resp.dragged() => {
                let t = ed.stored_t(n);
                if let Some(c) = ed.chapters.iter_mut().find(|c| c.id == id) { c.t = t; }
                ed.seek(n);
            }
            _ => if resp.clicked() || resp.dragged() { ed.seek(n); },
        }
    }
    if resp.drag_stopped() { if ed.drag_ch.take().is_some() { ed.sort(); } }
    if resp.hovered() && ed.chapters.iter().any(|c| ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| (x_of(c.t) - p.x).abs() <= 6.0)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    // barre de defilement de la vue zoomee
    if ed.zoom > 1.0 {
        let mut f = (ed.view_t0 / (dur - span).max(1e-9)) as f32;
        if ui.add(egui::Slider::new(&mut f, 0.0..=1.0).show_value(false)).changed() { ed.view_t0 = f as f64 * (dur - span).max(0.0); }
    }
}
