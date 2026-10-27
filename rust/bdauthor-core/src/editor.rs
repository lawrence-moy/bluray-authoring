//! Logique de l'editeur de mise en page, independante de l'interface : liste des pages, selection a la souris,
//! deplacement et redimensionnement avec aimantation, reprise d'un decor sur d'autres pages.
use crate::design::{Designs, Item, Kind, MenuType, PageDesign, PLANE};
use crate::model::ProjectModel;
use crate::project::paginate;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PageRef {
    Main(usize),
    /// (video, page)
    Chapters(usize, usize),
    /// Page libre du menu principal (0-based, apres les pages automatiques).
    Free(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageInfo {
    pub r: PageRef,
    pub menu: MenuType,
    pub page: usize,
    pub pages: usize,
    /// Videos (menu principal) ou chapitres (menu de chapitres) places sur cette page, par indice.
    pub items: Vec<usize>,
    pub video: Option<usize>,
}

impl PageInfo {
    pub fn count(&self) -> usize { self.items.len() }
    pub fn is_free(&self) -> bool { matches!(self.r, PageRef::Free(_)) }
    pub fn custom(&self, d: &Designs) -> bool {
        match self.r { PageRef::Main(p) => d.main.contains_key(&p), PageRef::Chapters(v, p) => d.chapters.contains_key(&(v, p)), PageRef::Free(f) => f < d.free.len() }
    }
    pub fn design<'a>(&self, d: &'a Designs) -> Option<&'a PageDesign> {
        match self.r { PageRef::Main(p) => d.main.get(&p), PageRef::Chapters(v, p) => d.chapters.get(&(v, p)), PageRef::Free(f) => d.free.get(f) }
    }
    pub fn design_mut<'a>(&self, d: &'a mut Designs) -> Option<&'a mut PageDesign> {
        match self.r { PageRef::Main(p) => d.main.get_mut(&p), PageRef::Chapters(v, p) => d.chapters.get_mut(&(v, p)), PageRef::Free(f) => d.free.get_mut(f) }
    }
    pub fn set_design(&self, d: &mut Designs, design: PageDesign) {
        match self.r {
            PageRef::Main(p) => { d.main.insert(p, design); }
            PageRef::Chapters(v, p) => { d.chapters.insert((v, p), design); }
            PageRef::Free(f) => { if let Some(x) = d.free.get_mut(f) { *x = design; } }
        }
    }
    pub fn remove_design(&self, d: &mut Designs) {
        match self.r {
            PageRef::Main(p) => { d.main.remove(&p); }
            PageRef::Chapters(v, p) => { d.chapters.remove(&(v, p)); }
            PageRef::Free(f) => { if f < d.free.len() { d.free.remove(f); d.page_removed(self.page); } }
        }
    }
    /// Mise en page de depart (grille automatique + titre) pour cette page.
    pub fn starting_point(&self, m: &ProjectModel) -> Result<PageDesign, String> {
        let title = match (self.menu, self.video) {
            (MenuType::Chapters, Some(v)) => format!("{} - chapitres", video_title(m, v)),
            _ => m.title.clone(),
        };
        PageDesign::starting_point(self.menu, self.count(), self.page, self.pages, &title)
    }
    /// Texte du selecteur de pages.
    pub fn label(&self, m: &ProjectModel) -> String {
        match (self.menu, self.video) {
            (MenuType::Chapters, Some(v)) => format!("Chapitres de « {} » — page {}/{}", video_title(m, v), self.page + 1, self.pages),
            _ if self.is_free() => format!("Page libre — page {}/{}", self.page + 1, self.pages),
            _ => format!("Menu principal — page {}/{}", self.page + 1, self.pages),
        }
    }
}

pub fn video_title(m: &ProjectModel, i: usize) -> String {
    let v = &m.videos[i];
    if !v.title.is_empty() { return v.title.clone(); }
    std::path::Path::new(&v.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Pages de menu du projet. `durations` : duree (secondes) des fichiers deja sondes ; sans elle, les chapitres automatiques sont ignores.
pub fn list_pages(m: &ProjectModel, durations: &HashMap<String, f64>) -> Vec<PageInfo> {
    let mut out = Vec::new();
    if m.videos.is_empty() { return out; }
    let main = paginate(m.videos.len(), m.par_page).unwrap_or_else(|_| vec![(0..m.videos.len()).collect()]);
    let auto = main.len();
    let np = auto + m.designs.free.len(); // les pages libres prolongent la suite des pages du menu principal
    for (p, idxs) in main.into_iter().enumerate() {
        out.push(PageInfo { r: PageRef::Main(p), menu: MenuType::Main, page: p, pages: np, items: idxs, video: None });
    }
    for f in 0..m.designs.free.len() {
        // elements d'une page libre = toutes les videos du projet (a choisir pour chaque vignette)
        out.push(PageInfo { r: PageRef::Free(f), menu: MenuType::Main, page: auto + f, pages: np, items: (0..m.videos.len()).collect(), video: None });
    }
    for (i, v) in m.videos.iter().enumerate() {
        let ch = v.chapter_list(durations.get(&v.file).copied());
        if ch.len() < 2 { continue; }
        let pg = paginate(ch.len(), m.par_page).unwrap_or_else(|_| vec![(0..ch.len().min(12)).collect()]);
        let np = pg.len();
        for (q, idxs) in pg.into_iter().enumerate() {
            out.push(PageInfo { r: PageRef::Chapters(i, q), menu: MenuType::Chapters, page: q, pages: np, items: idxs, video: Some(i) });
        }
    }
    out
}

// ------------------------------------------------------------------ souris
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Handle { Body, TopLeft, TopRight, BottomLeft, BottomRight }

/// Element (et poignee) sous le point (x, y) du plan 1920x1080. Les poignees d'angle de l'element selectionne sont prioritaires ;
/// `r` = rayon de saisie des poignees en unites du plan.
pub fn hit_test(items: &[Item], x: f32, y: f32, selected: Option<usize>, r: f32) -> Option<(usize, Handle)> {
    if let Some(s) = selected.and_then(|s| items.get(s).map(|it| (s, it))) {
        let (i, it) = s;
        let (l, t, rr, b) = (it.x as f32, it.y as f32, (it.x + it.w) as f32, (it.y + it.h) as f32);
        for (hx, hy, h) in [(l, t, Handle::TopLeft), (rr, t, Handle::TopRight), (l, b, Handle::BottomLeft), (rr, b, Handle::BottomRight)] {
            if (x - hx).abs() <= r && (y - hy).abs() <= r { return Some((i, h)); }
        }
    }
    items.iter().enumerate().rev().find(|(_, it)| x >= it.x as f32 && x < (it.x + it.w) as f32 && y >= it.y as f32 && y < (it.y + it.h) as f32).map(|(i, _)| (i, Handle::Body))
}

fn snap(v: f32, grid: i32) -> i32 { if grid > 1 { ((v / grid as f32).round() as i32) * grid } else { v.round() as i32 } }

/// Nouveau rectangle (x, y, w, h) apres un glissement de (dx, dy) unites du plan depuis `orig`. Taille minimale `min`.
/// Le deplacement garde l'element dans l'ecran ; `grid` > 1 aimante les bords sur une grille.
pub fn dragged_rect(orig: (i32, i32, i32, i32), handle: Handle, dx: f32, dy: f32, grid: i32, min: i32) -> (i32, i32, i32, i32) {
    let (ox, oy, ow, oh) = orig;
    match handle {
        Handle::Body => {
            let x = snap(ox as f32 + dx, grid).clamp(0.min(PLANE.0 - ow), (PLANE.0 - ow).max(0));
            let y = snap(oy as f32 + dy, grid).clamp(0.min(PLANE.1 - oh), (PLANE.1 - oh).max(0));
            (x, y, ow, oh)
        }
        _ => {
            let (mut l, mut t, mut r, mut b) = (ox, oy, ox + ow, oy + oh);
            if matches!(handle, Handle::TopLeft | Handle::BottomLeft) { l = snap(ox as f32 + dx, grid).clamp(0, r - min); }
            if matches!(handle, Handle::TopRight | Handle::BottomRight) { r = snap((ox + ow) as f32 + dx, grid).clamp(l + min, PLANE.0); }
            if matches!(handle, Handle::TopLeft | Handle::TopRight) { t = snap(oy as f32 + dy, grid).clamp(0, b - min); }
            if matches!(handle, Handle::BottomLeft | Handle::BottomRight) { b = snap((oy + oh) as f32 + dy, grid).clamp(t + min, PLANE.1); }
            (l, t, r - l, b - t)
        }
    }
}

/// Applique un rectangle a un element ; pour une vignette, la hauteur du libelle est conservee.
pub fn set_rect(it: &mut Item, orig: &Item, rect: (i32, i32, i32, i32)) {
    it.x = rect.0; it.y = rect.1; it.w = rect.2; it.h = rect.3;
    if let (Kind::Slot { thumb_h, .. }, Kind::Slot { thumb_h: o, .. }) = (&mut it.kind, &orig.kind) {
        let label_h = (orig.h - o).max(0);
        *thumb_h = (rect.3 - label_h).clamp(8.min(rect.3), rect.3);
    }
}

/// Deplace un element dans la liste (ordre de superposition pour les decors) ; retourne son nouvel indice.
pub fn move_in_list(d: &mut PageDesign, idx: usize, up: bool) -> usize {
    let to = if up { idx.checked_sub(1) } else { Some(idx + 1).filter(|p| *p < d.items.len()) };
    match to { Some(t) => { d.items.swap(idx, t); t } None => idx }
}

/// Met l'element au premier plan (fin de liste) ; retourne son nouvel indice.
pub fn to_front(d: &mut PageDesign, idx: usize) -> usize {
    if idx >= d.items.len() { return idx; }
    let it = d.items.remove(idx);
    d.items.push(it);
    d.items.len() - 1
}

/// Met l'element a l'arriere-plan (debut de liste) ; retourne son nouvel indice (0).
pub fn to_back(d: &mut PageDesign, idx: usize) -> usize {
    if idx >= d.items.len() { return idx; }
    let it = d.items.remove(idx);
    d.items.insert(0, it);
    0
}

/// Reprend sur `target` les textes, images et boutons de navigation de `src`, dans l'ordre de superposition de `src`
/// (les vignettes de `target` restent telles quelles, a la place des vignettes de `src`).
/// Les boutons de navigation absents de `src` mais presents dans `target_default` (grille automatique de la page cible, par exemple
/// « Precedent » qui n'existe pas en page 1) sont ajoutes a leur place par defaut.
pub fn copy_decor(src: &PageDesign, target: PageDesign, target_default: &PageDesign) -> PageDesign {
    let mut slots = target.items.into_iter().filter(|i| matches!(i.kind, Kind::Slot { .. }));
    let mut items = Vec::new();
    for it in &src.items {
        if matches!(it.kind, Kind::Slot { .. }) {
            if let Some(s) = slots.next() { items.push(s); }
        } else {
            items.push(it.clone());
        }
    }
    items.extend(slots);
    for d in &target_default.items {
        if let Kind::Nav { action, .. } = &d.kind {
            if !items.iter().any(|i| matches!(&i.kind, Kind::Nav { action: a, .. } if a == action)) {
                items.push(d.clone());
            }
        }
    }
    PageDesign { items }
}

/// Ajoute une page libre vierge a la fin du menu principal et retourne son indice. La page precedente (automatique non
/// personnalisee : deja geree par la grille ; personnalisee ou libre : on ajoute un bouton « Suivant » s'il manque) mene a la nouvelle.
pub fn add_free_page(m: &mut ProjectModel) -> usize {
    let auto = paginate(m.videos.len(), m.par_page).map(|g| g.len()).unwrap_or(1);
    let f = m.designs.free.len();
    let page = auto + f;
    let prev: Option<&mut PageDesign> = if f > 0 { m.designs.free.get_mut(f - 1) } else { m.designs.main.get_mut(&(auto - 1)) };
    if let Some(prev) = prev {
        if !prev.items.iter().any(|i| matches!(&i.kind, Kind::Nav { action: crate::design::NavAction::Next, .. })) {
            prev.items.push(Item::nav(crate::design::NavAction::Next, 1000, 1080 - 60 - 80, 360, 80));
        }
    }
    m.designs.free.push(PageDesign::blank_free(page));
    f
}

// ------------------------------------------------------------------ annuler / refaire
/// Historique des mises en page (annuler / refaire). `observe` est appele a chaque image : il detecte tout changement
/// et l'enregistre ; les changements rapproches (< `merge_ms`, par exemple des fleches du clavier ou une saisie de texte) forment une seule etape.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Designs>,
    redo: Vec<Designs>,
    last: Designs,
    last_ms: u64,
}

impl History {
    const LIMIT: usize = 200;
    const MERGE_MS: u64 = 600;

    /// Repart de `cur` avec un historique vide (ouverture de projet, deplacement ou suppression de video...).
    pub fn reset(&mut self, cur: &Designs) {
        self.undo.clear();
        self.redo.clear();
        self.last = cur.clone();
        self.last_ms = 0;
    }
    /// A appeler a chaque image, sauf pendant un glissement (qui ne forme alors qu'une seule etape).
    pub fn observe(&mut self, cur: &Designs, now_ms: u64) {
        if *cur == self.last { return; }
        let merge = !self.undo.is_empty() && now_ms.saturating_sub(self.last_ms) < Self::MERGE_MS && self.redo.is_empty();
        if !merge {
            self.undo.push(self.last.clone());
            if self.undo.len() > Self::LIMIT { self.undo.remove(0); }
        }
        self.redo.clear();
        self.last = cur.clone();
        self.last_ms = now_ms;
    }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
    /// Annule la derniere etape ; `cur` doit avoir ete observe juste avant.
    pub fn undo(&mut self, cur: &mut Designs) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(cur.clone());
        *cur = prev;
        self.last = cur.clone();
        self.last_ms = 0;
        true
    }
    pub fn redo(&mut self, cur: &mut Designs) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(cur.clone());
        *cur = next;
        self.last = cur.clone();
        self.last_ms = 0;
        true
    }
}
