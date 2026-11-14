//! Fenetre « Verifier les menus » : constats du verificateur de coherence (graphe pages / boutons / videos), filtres par
//! gravite, clic sur un constat pour aller a la page, a l'element ou a la video concernes, et carte du disque.
use crate::i18n::{tr, trf};
use crate::{App, Mode};
use bdauthor_core::coherence::{check, Dest, Place, Report, Severity};
use bdauthor_core::editor::PageRef;
use eframe::egui::{self, Color32, RichText};

const RED: Color32 = Color32::from_rgb(230, 100, 90);
const AMBER: Color32 = Color32::from_rgb(230, 175, 70);
const GREY: Color32 = Color32::from_rgb(150, 150, 150);

fn sev_color(s: Severity) -> Color32 { match s { Severity::Error => RED, Severity::Warning => AMBER, Severity::Info => GREY } }
fn sev_icon(s: Severity) -> &'static str { match s { Severity::Error => "✖", Severity::Warning => "⚠", Severity::Info => "ℹ" } }

impl App {
    /// Analyse du projet en cours (rapide : refaite a chaque image de la fenetre).
    pub(crate) fn coherence_report(&self) -> Report {
        check(&self.model, &|v| self.model.videos.get(v).and_then(|ve| self.durations.get(&ve.file).copied()))
    }

    /// Montre ce que vise un constat.
    fn coherence_go(&mut self, place: Place) {
        match place {
            Place::Page(k) => {
                self.mode = Mode::MiseEnPage;
                self.cur = Some(PageRef(k));
                self.sel_item = None;
                self.extra_sel.clear();
            }
            Place::Item(k, i) => {
                self.mode = Mode::MiseEnPage;
                self.cur = Some(PageRef(k));
                self.sel_item = Some(i);
                self.extra_sel.clear();
            }
            Place::Video(v) => {
                self.mode = Mode::Projet;
                self.sel = Some(v);
            }
        }
    }

    pub(crate) fn coherence_window(&mut self, ctx: &egui::Context) {
        if !self.show_coherence { return; }
        let mut open = true;
        let r = self.coherence_report();
        let mut go: Option<Place> = None;
        egui::Window::new(tr("Check the menus")).id(egui::Id::new("verif_coherence")).open(&mut open).default_size([640.0, 560.0]).show(ctx, |ui| {
            // resume
            ui.horizontal(|ui| {
                for (s, label) in [(Severity::Error, tr("Errors")), (Severity::Warning, tr("Warnings")), (Severity::Info, tr("Information"))] {
                    let n = r.count(s);
                    let txt = RichText::new(format!("{} {n} {label}", sev_icon(s))).strong().color(if n > 0 { sev_color(s) } else { GREY });
                    let on = self.coherence_filter == Some(s);
                    if ui.selectable_label(on, txt).on_hover_text(tr("Show only these findings (click again: show all)")).clicked() {
                        self.coherence_filter = if on { None } else { Some(s) };
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(tr("updated as you edit")).small().weak());
                });
            });
            ui.separator();
            // constats
            let shown: Vec<(usize, &bdauthor_core::coherence::Finding)> = r.findings.iter().enumerate().filter(|(_, f)| self.coherence_filter.map_or(true, |s| f.severity == s)).collect();
            let list_h = (ui.available_height() * 0.6).max(120.0);
            egui::ScrollArea::vertical().id_salt("constats").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
                if r.findings.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("✔ {}", tr("No problem found in the menus."))).color(Color32::from_rgb(110, 190, 110)));
                } else if shown.is_empty() {
                    ui.label(RichText::new(tr("Nothing in this category.")).weak());
                }
                for (k, f) in shown {
                    let sel = self.coherence_sel == Some(k);
                    let where_ = match f.place {
                        Place::Page(p) => bdauthor_core::coherence::page_name(&self.model, p),
                        Place::Item(p, i) => trf!("{} · item {}", bdauthor_core::coherence::page_name(&self.model, p), i + 1),
                        Place::Video(v) => trf!("Video {}", v + 1),
                    };
                    let resp = ui.horizontal(|ui| {
                        ui.label(RichText::new(sev_icon(f.severity)).color(sev_color(f.severity)).size(16.0));
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(&f.msg).wrap());
                            ui.label(RichText::new(trf!("{} · rule: {}", where_, f.rule.label())).small().weak());
                        });
                    }).response;
                    let resp = ui.interact(resp.rect, egui::Id::new(("constat", k)), egui::Sense::click());
                    if sel || resp.hovered() {
                        let c = if sel { ui.visuals().selection.stroke.color } else { ui.visuals().widgets.hovered.bg_stroke.color };
                        ui.painter().rect_stroke(resp.rect.expand(2.0), 4.0, egui::Stroke::new(1.0, c));
                    }
                    if resp.on_hover_text(tr("Click: show it in the editor")).clicked() {
                        self.coherence_sel = Some(k);
                        go = Some(f.place);
                    }
                    ui.separator();
                }
            });
            // carte du disque
            if !r.generated && !r.pages.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(tr("Map of the disc")).strong());
                egui::ScrollArea::vertical().id_salt("carte").auto_shrink([false, true]).show(ui, |ui| {
                    egui::Grid::new("carte_disque").num_columns(2).spacing([10.0, 4.0]).striped(true).show(ui, |ui| {
                        for (k, p) in r.pages.iter().enumerate() {
                            let mut name = p.name.clone();
                            if k == r.home { name = format!("⌂ {name}"); }
                            let col = if p.reachable { ui.visuals().text_color() } else { RED };
                            if ui.add(egui::Label::new(RichText::new(name).color(col)).sense(egui::Sense::click())).on_hover_text(tr("Click: open this page")).clicked() { go = Some(Place::Page(k)); }
                            let mut parts: Vec<String> = p.exits.iter().map(|e| {
                                let d = match e.dest {
                                    Dest::Page(q) => r.pages.get(q).map_or_else(|| trf!("Page {}", q + 1), |x| x.name.clone()),
                                    Dest::Video(v) => trf!("Video {}", v + 1),
                                    Dest::PlayAll => tr("Play all").to_string(),
                                };
                                match e.auto_s { Some(s) => trf!("{} (auto {} s)", d, s), None => d }
                            }).collect();
                            if parts.is_empty() { parts.push(tr("no exit").to_string()); }
                            let mut line = format!("→ {}", parts.join(" · "));
                            if !p.reachable { line = format!("{line}   ← {}", tr("nothing leads here")); }
                            ui.label(RichText::new(line).color(if p.reachable { ui.visuals().weak_text_color() } else { RED }));
                            ui.end_row();
                        }
                        for (v, n) in r.videos.iter().enumerate() {
                            let orphan = n.from.is_empty() || n.from.iter().all(|k| !r.pages.get(*k).is_some_and(|p| p.reachable));
                            let col = if orphan { AMBER } else { ui.visuals().text_color() };
                            if ui.add(egui::Label::new(RichText::new(trf!("Video {}", v + 1)).color(col)).sense(egui::Sense::click())).on_hover_text(&n.title).clicked() { go = Some(Place::Video(v)); }
                            let names = |l: &[usize]| l.iter().map(|k| r.pages.get(*k).map_or(String::new(), |p| p.name.clone())).collect::<Vec<_>>().join(", ");
                            let line = if n.from.is_empty() { tr("played by no button").to_string() } else { trf!("played from {} · return: {}", names(&n.from), names(&n.returns)) };
                            ui.label(RichText::new(line).color(if orphan { AMBER } else { ui.visuals().weak_text_color() }));
                            ui.end_row();
                        }
                    });
                });
            }
        });
        if let Some(p) = go { self.coherence_go(p); }
        if !open { self.show_coherence = false; }
    }
}
