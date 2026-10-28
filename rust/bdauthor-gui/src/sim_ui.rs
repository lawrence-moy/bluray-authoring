//! Simulation de la telecommande dans l'editeur : fleches pour passer d'un bouton a l'autre (meme regle de voisinage que le disque),
//! Entree pour valider (etat « active » visible un instant), puis ouverture de la page visee ou lecture simulee de la video.
use crate::App;
use bdauthor_core::design::Target;
use bdauthor_core::editor::{main_page_of_video, page_of_target, sim_buttons, video_title, PageInfo, PageRef, SimAction, SimButton};
use eframe::egui::{self, Color32, Key, Rect};
use std::time::{Duration, Instant};

/// Duree d'affichage de l'etat « active » avant d'executer l'action.
const PRESS: Duration = Duration::from_millis(350);

#[derive(Clone, Default)]
pub(crate) struct Sim {
    /// bouton selectionne (indice dans la liste des boutons de la page)
    pub btn: usize,
    /// validation en cours (etat « active » affiche)
    pub pressed: Option<Instant>,
    /// lecture simulee : (texte affiche, page ou revenir a la fin)
    pub playing: Option<(String, Option<PageRef>)>,
}

impl App {
    fn has_chapters(&self, pages: &[PageInfo], v: usize) -> bool {
        pages.iter().any(|p| matches!(p.r, PageRef::Chapters(x, _) if x == v))
    }

    pub(crate) fn sim_page_buttons(&self, info: &PageInfo) -> Vec<SimButton> {
        let Some(d) = info.design(&self.model.designs).cloned().or_else(|| info.starting_point(&self.model).ok()) else { return vec![] };
        let pages = self.pages();
        sim_buttons(info, &d, &self.model.designs.style, self.model.videos.len(), &|v| self.has_chapters(&pages, v))
    }

    /// L'etat « active » est-il affiche ?
    pub(crate) fn sim_pressed(&self) -> bool {
        self.sim.as_ref().map_or(false, |s| s.pressed.is_some())
    }

    pub(crate) fn sim_goto(&mut self, r: PageRef) {
        self.cur = Some(r);
        if let Some(s) = self.sim.as_mut() {
            s.btn = 0;
            s.pressed = None;
        }
        self.sel_item = None;
        self.edit_gen += 1;
    }

    /// Execute l'action d'un bouton valide.
    fn sim_do(&mut self, info: &PageInfo, a: SimAction) {
        let pages = self.pages();
        // a la fin d'une video : son menu de chapitres (si l'option est active et qu'il existe), sinon sa page du menu principal
        let back_of = |v: usize| -> Option<PageRef> {
            if self.model.back_to_chapters && self.has_chapters(&pages, v) { Some(PageRef::Chapters(v, 0)) } else { main_page_of_video(&pages, v) }
        };
        let mut play: Option<(String, Option<PageRef>)> = None;
        let mut go: Option<PageRef> = None;
        match a {
            SimAction::Play(v) | SimAction::Go(Target::Video(v)) => {
                play = Some((format!("Lecture de « {} »", video_title(&self.model, v)), back_of(v)));
            }
            SimAction::PlayChapter(v, c) => {
                let name = self.model.videos.get(v).and_then(|x| x.chapter_list(self.durations.get(&x.file).copied()).get(c).map(|t| t.1.clone())).unwrap_or_else(|| format!("chapitre {}", c + 1));
                play = Some((format!("Lecture de « {} » à partir de « {name} »", video_title(&self.model, v)), back_of(v)));
            }
            SimAction::Go(t) => go = page_of_target(&pages, t),
            SimAction::Prev | SimAction::Next => {
                let d = if a == SimAction::Next { 1 } else { -1 };
                go = match info.r {
                    PageRef::Chapters(v, q) => (q as i64 + d >= 0).then(|| PageRef::Chapters(v, (q as i64 + d) as usize)).filter(|r| pages.iter().any(|p| p.r == *r)),
                    _ => (info.page as i64 + d >= 0).then(|| page_of_target(&pages, Target::Main((info.page as i64 + d) as usize))).flatten(),
                };
            }
            SimAction::Back => go = info.video.and_then(|v| main_page_of_video(&pages, v)),
        }
        if let Some(p) = play {
            if let Some(s) = self.sim.as_mut() { s.playing = Some(p); }
            self.edit_gen += 1;
        } else if let Some(r) = go {
            self.sim_goto(r);
        } else {
            self.notice = "Simulation : la destination de ce bouton n'existe pas.".into();
        }
    }

    /// Dessin et commandes de la simulation par-dessus l'apercu (`rect` : zone de l'apercu, `k` : pixels d'ecran par pixel du plan 1920).
    pub(crate) fn sim_ui(&mut self, ctx: &egui::Context, info: &PageInfo, resp: &egui::Response, painter: &egui::Painter, rect: Rect, k: f32) {
        let Some(mut sim) = self.sim.clone() else { return };
        // les touches vont a la simulation, pas a un bouton de l'interface qui aurait garde le focus
        ctx.memory_mut(|m| if let Some(id) = m.focused() { m.surrender_focus(id) });
        let help = |painter: &egui::Painter, text: &str| {
            let r = Rect::from_min_max(rect.left_bottom() - egui::vec2(0.0, 26.0), rect.right_bottom());
            painter.rect_filled(r, 0.0, Color32::from_black_alpha(170));
            painter.text(r.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(14.0), Color32::WHITE);
        };
        let keys = ctx.input(|i| {
            (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowLeft), i.key_pressed(Key::ArrowRight),
             i.key_pressed(Key::Enter) || i.key_pressed(Key::Space), i.key_pressed(Key::Escape))
        });
        let (up, down, left, right, ok, esc) = keys;

        // lecture simulee d'une video
        if let Some((what, back)) = sim.playing.clone() {
            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(220));
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, format!("▶  {what}"), egui::FontId::proportional(22.0), Color32::WHITE);
            help(painter, "Fin de la vidéo : Entrée, Échap ou clic");
            if ok || esc || resp.clicked() {
                sim.playing = None;
                self.sim = Some(sim);
                match back { Some(r) => self.sim_goto(r), None => self.edit_gen += 1 }
            }
            return;
        }

        let buttons = self.sim_page_buttons(info);
        if buttons.is_empty() {
            help(painter, "Aucun bouton sur cette page · Échap : quitter la simulation");
            self.sel_item = None;
            if esc { self.sim = None; self.edit_gen += 1; } else { self.sim = Some(sim); }
            return;
        }
        if sim.btn >= buttons.len() { sim.btn = 0; }

        let mut action = None;
        if let Some(t) = sim.pressed {
            if t.elapsed() >= PRESS {
                sim.pressed = None;
                action = Some(buttons[sim.btn].action);
            } else {
                ctx.request_repaint_after(PRESS.saturating_sub(t.elapsed()));
            }
        } else if esc {
            self.sim = None;
            self.sel_item = None;
            self.edit_gen += 1;
            return;
        } else {
            let n = buttons[sim.btn].neighbors;
            if up { sim.btn = n.0; } else if down { sim.btn = n.1; } else if left { sim.btn = n.2; } else if right { sim.btn = n.3; }
            if ok { sim.pressed = Some(Instant::now()); ctx.request_repaint_after(PRESS); }
            // clic : selection et validation du bouton sous la souris
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let (px, py) = ((pos.x - rect.min.x) / k, (pos.y - rect.min.y) / k);
                    if let Some(j) = buttons.iter().rposition(|b| px >= b.rect.0 as f32 && px < (b.rect.0 + b.rect.2) as f32 && py >= b.rect.1 as f32 && py < (b.rect.1 + b.rect.3) as f32) {
                        sim.btn = j;
                        sim.pressed = Some(Instant::now());
                        ctx.request_repaint_after(PRESS);
                    }
                }
            }
        }
        self.sel_item = Some(buttons[sim.btn].item);
        self.sim = Some(sim);
        help(painter, &format!("Simulation · bouton {} sur {} · Flèches : se déplacer · Entrée : valider · Échap : quitter", self.sim.as_ref().map_or(0, |s| s.btn) + 1, buttons.len()));
        if let Some(a) = action {
            self.sim_do(info, a);
        }
    }
}
