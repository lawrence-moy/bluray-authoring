//! Simulation de la telecommande dans l'editeur : fleches pour passer d'un bouton a l'autre (meme regle de voisinage que le disque),
//! Entree pour valider (etat « active » visible un instant), puis ouverture de la page visee ou lecture simulee de la video.
use crate::App;
use bdauthor_core::design::{SlotReturn, SubChoice, Target};
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
        // pages generiques : une vignette lit sa video (les pages de chapitres sont des liens choisis)
        let generic = self.model.designs.generic;
        sim_buttons(info, &d, &self.model.designs.style, self.model.videos.len(), &|v| !generic && self.has_chapters(&pages, v))
    }

    /// Page d'accueil (touche TOP MENU) : celle choisie, sinon la page 1 du menu principal.
    pub(crate) fn home_page(&self) -> Option<PageRef> {
        let pages = self.pages();
        self.model.designs.home.and_then(|h| page_of_target(&pages, h)).or_else(|| page_of_target(&pages, Target::Main(0)))
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
    fn sim_do(&mut self, info: &PageInfo, a: SimAction, ret: SlotReturn, sub: SubChoice) {
        let pages = self.pages();
        // sous-titres reglés par le bouton, rappeles dans le texte de la lecture simulee
        let sub_txt = |v: usize| -> String {
            match sub {
                SubChoice::Keep => String::new(),
                SubChoice::Off => " — sans sous-titres".to_string(),
                SubChoice::Track(k) => {
                    let lang = self.model.videos.get(v).and_then(|x| x.subs.get(k)).map(|t| t.lang.clone());
                    match lang { Some(l) => format!(" — sous-titres : piste {} ({l})", k + 1), None => format!(" — sous-titres : piste {} absente", k + 1) }
                }
                SubChoice::Lang(id) => {
                    let name = self.model.language_name(id);
                    let has = self.model.videos.get(v).map_or(false, |x| x.subs.iter().any(|t| t.lang_id == id));
                    if has { format!(" — sous-titres : {name}") } else { format!(" — sous-titres : {name} (absents de cette vidéo : rien ne s'affiche)") }
                }
            }
        };
        let generic = self.model.designs.generic;
        // pages generiques : retour choisi par la vignette (page d'origine par defaut)
        let generic_back = match ret {
            SlotReturn::Origin => Some(info.r),
            SlotReturn::Home => self.home_page(),
            SlotReturn::Page(p) => page_of_target(&pages, Target::Main(p)).or(Some(info.r)),
        };
        // a la fin d'une video : son menu de chapitres (si l'option est active et qu'il existe), sinon sa page du menu principal
        let back_of = |v: usize| -> Option<PageRef> {
            if generic { return generic_back; }
            if self.model.back_to_chapters && self.has_chapters(&pages, v) { Some(PageRef::Chapters(v, 0)) } else { main_page_of_video(&pages, v) }
        };
        let mut play: Option<(String, Option<PageRef>)> = None;
        let mut go: Option<PageRef> = None;
        match a {
            SimAction::Play(v) | SimAction::Go(Target::Video(v)) => {
                play = Some((format!("Lecture de « {} »{}", video_title(&self.model, v), sub_txt(v)), back_of(v)));
            }
            SimAction::PlayChapter(v, c) => {
                let name = self.model.videos.get(v).and_then(|x| x.chapter_list(self.durations.get(&x.file).copied()).get(c).map(|t| t.1.clone())).unwrap_or_else(|| format!("chapitre {}", c + 1));
                play = Some((format!("Lecture de « {} » à partir de « {name} »{}", video_title(&self.model, v), sub_txt(v)), back_of(v)));
            }
            SimAction::PlayFrom(v, t) => {
                let s = t.round().max(0.0) as i64;
                play = Some((format!("Lecture de « {} » à partir de {}:{:02}{}", video_title(&self.model, v), s / 60, s % 60, sub_txt(v)), back_of(v)));
            }
            SimAction::PlayAllVideos => {
                // le disque revient a la page d'ou « Tout lire » a ete lance
                play = Some(("Lecture de toutes les vidéos à la suite".to_string(), if generic { Some(info.r) } else { self.home_page() }));
            }
            SimAction::Go(Target::Stay) => {
                // aucune navigation : seul le reglage des sous-titres a lieu
                self.notice = match sub {
                    SubChoice::Keep => "Simulation : ce bouton ne fait rien.".to_string(),
                    SubChoice::Off => "Simulation : sous-titres masqués.".to_string(),
                    SubChoice::Track(k) => format!("Simulation : sous-titres réglés sur la piste {}.", k + 1),
                    SubChoice::Lang(id) => format!("Simulation : sous-titres réglés sur « {} ».", self.model.language_name(id)),
                };
                self.edit_gen += 1;
                return;
            }
            SimAction::Go(Target::Audio(id)) => {
                // aucune navigation : seule la langue audio change (pour les videos lues ensuite)
                let name = self.model.language_name(id);
                let has = self.model.videos.iter().any(|v| v.audio.iter().any(|t| t.lang_id == id));
                self.notice = if has { format!("Simulation : langue audio réglée sur « {name} ».") } else { format!("Simulation : aucune vidéo n'a d'audio en « {name} » : rien ne change.") };
                self.edit_gen += 1;
                return;
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
             i.key_pressed(Key::Enter) || i.key_pressed(Key::Space), i.key_pressed(Key::Escape), i.key_pressed(Key::Home) || i.key_pressed(Key::M))
        });
        let (up, down, left, right, ok, esc, top_menu) = keys;
        // touche TOP MENU (Origine ou M) : accueil, meme pendant une lecture
        if top_menu {
            sim.playing = None;
            sim.pressed = None;
            self.sim = Some(sim);
            if let Some(h) = self.home_page() { self.sim_goto(h); }
            return;
        }

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
                action = Some((buttons[sim.btn].action, buttons[sim.btn].ret, buttons[sim.btn].sub));
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
        help(painter, &format!("Simulation · bouton {} sur {} · Flèches : se déplacer · Entrée : valider · M : TOP MENU · Échap : quitter", self.sim.as_ref().map_or(0, |s| s.btn) + 1, buttons.len()));
        if let Some((a, ret, sub)) = action {
            self.sim_do(info, a, ret, sub);
        }
    }
}
