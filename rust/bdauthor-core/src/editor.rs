//! Logique de l'editeur de mise en page, independante de l'interface : liste des pages, selection a la souris,
//! deplacement et redimensionnement avec aimantation, reprise d'un decor sur d'autres pages.
use crate::design::{Designs, Item, Kind, Layer, MenuType, NavAction, PageDesign, Target, PLANE};
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
        if m.designs.generic {
            let name = m.designs.free.get(self.page).map(|d| d.name.clone()).unwrap_or_default();
            return if name.is_empty() { format!("Page {}", self.page + 1) } else { format!("{}. {name}", self.page + 1) };
        }
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
    if m.designs.generic {
        // pages generiques : toutes personnalisees, dans l'ordre de la liste
        let np = m.designs.free.len();
        return (0..np).map(|k| PageInfo { r: PageRef::Free(k), menu: MenuType::Main, page: k, pages: np, items: vec![], video: None }).collect();
    }
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
    hit_test_where(items, x, y, selected, r, &|_| true)
}

/// Comme `hit_test`, en ne considerant que les elements `usable` (calques visibles et non verrouilles).
pub fn hit_test_where(items: &[Item], x: f32, y: f32, selected: Option<usize>, r: f32, usable: &dyn Fn(usize) -> bool) -> Option<(usize, Handle)> {
    if let Some(s) = selected.filter(|&s| usable(s)).and_then(|s| items.get(s).map(|it| (s, it))) {
        let (i, it) = s;
        let (l, t, rr, b) = (it.x as f32, it.y as f32, (it.x + it.w) as f32, (it.y + it.h) as f32);
        for (hx, hy, h) in [(l, t, Handle::TopLeft), (rr, t, Handle::TopRight), (l, b, Handle::BottomLeft), (rr, b, Handle::BottomRight)] {
            if (x - hx).abs() <= r && (y - hy).abs() <= r { return Some((i, h)); }
        }
    }
    items.iter().enumerate().rev().find(|(i, it)| usable(*i) && x >= it.x as f32 && x < (it.x + it.w) as f32 && y >= it.y as f32 && y < (it.y + it.h) as f32).map(|(i, _)| (i, Handle::Body))
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

/// Deplace un element d'un cran dans la liste (ordre de superposition), sans sortir de son calque ; retourne son nouvel indice.
/// `up` : vers l'arriere-plan (debut de liste).
pub fn move_in_list(d: &mut PageDesign, idx: usize, up: bool) -> usize {
    if idx >= d.items.len() { return idx; }
    let r = d.layer_range(d.layer_pos(idx));
    let to = if up { idx.checked_sub(1).filter(|t| *t >= r.start) } else { Some(idx + 1).filter(|t| *t < r.end) };
    match to { Some(t) => { d.items.swap(idx, t); t } None => idx }
}

/// Met l'element au premier plan de son calque (de la page, sans calques) ; retourne son nouvel indice.
pub fn to_front(d: &mut PageDesign, idx: usize) -> usize {
    if idx >= d.items.len() { return idx; }
    let end = d.layer_range(d.layer_pos(idx)).end;
    let it = d.items.remove(idx);
    d.items.insert(end - 1, it);
    end - 1
}

/// Met l'element a l'arriere-plan de son calque (de la page, sans calques) ; retourne son nouvel indice.
pub fn to_back(d: &mut PageDesign, idx: usize) -> usize {
    if idx >= d.items.len() { return idx; }
    let start = d.layer_range(d.layer_pos(idx)).start;
    let it = d.items.remove(idx);
    d.items.insert(start, it);
    start
}

// ------------------------------------------------------------------ calques de l'editeur
/// Ajoute un element au premier plan du calque `layer` (identifiant ; 0 : premier calque, ou fin de liste sans calques) ;
/// retourne son indice.
pub fn insert_item(d: &mut PageDesign, mut it: Item, layer: u32) -> usize {
    if d.layers.is_empty() {
        it.layer = 0;
        d.items.push(it);
        return d.items.len() - 1;
    }
    let pos = d.layers.iter().position(|l| l.id == layer).unwrap_or(0);
    it.layer = d.layers[pos].id;
    let end = d.layer_range(pos).end;
    d.items.insert(end, it);
    end
}

/// Ajoute un calque au premier plan et retourne son identifiant. Le premier calque cree recoit d'abord tous les elements existants
/// (« Calque 1 »).
pub fn add_layer(d: &mut PageDesign) -> u32 {
    if d.layers.is_empty() {
        d.layers.push(Layer { id: 1, name: "Calque 1".into(), ..Default::default() });
        for it in &mut d.items { it.layer = 1; }
    }
    let id = d.layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
    let mut k = d.layers.len() + 1;
    while d.layers.iter().any(|l| l.name == format!("Calque {k}")) { k += 1; }
    d.layers.push(Layer { id, name: format!("Calque {k}"), ..Default::default() });
    id
}

/// Supprime le calque `id`. Avec `keep_items`, ses elements rejoignent le calque juste derriere (ou devant s'il etait au fond) ;
/// sinon ils sont supprimes. Sans calque restant, la page revient a une liste simple.
pub fn remove_layer(d: &mut PageDesign, id: u32, keep_items: bool) {
    let Some(pos) = d.layers.iter().position(|l| l.id == id) else { return };
    if !keep_items { d.items.retain(|it| it.layer != id); }
    d.layers.remove(pos);
    if let Some(dest) = d.layers.get(pos.saturating_sub(1)).map(|l| l.id) {
        // derriere : ses elements passent au premier plan du calque de destination (ils gardent leur place a l'ecran)
        for it in &mut d.items { if it.layer == id { it.layer = dest; } }
    }
    d.normalize_layers();
}

/// Deplace le calque en position `from` pour qu'il soit en position `to` (positions de l'arriere-plan au premier plan) ;
/// ses elements suivent.
pub fn move_layer(d: &mut PageDesign, from: usize, to: usize) {
    if from >= d.layers.len() || from == to { return; }
    let l = d.layers.remove(from);
    d.layers.insert(to.min(d.layers.len()), l);
    d.normalize_layers();
}

/// Glisser-deposer : met l'element `from` dans le calque `layer` (identifiant, ignore sans calques) juste devant l'element qui
/// est en `before` dans la liste actuelle (`before` = indice ou s'inserer ; `items.len()` : fin). Retourne son nouvel indice.
pub fn move_item(d: &mut PageDesign, from: usize, before: usize, layer: u32) -> usize {
    if from >= d.items.len() { return from; }
    let mut it = d.items.remove(from);
    if !d.layers.is_empty() { it.layer = layer; }
    let at = if before > from { before - 1 } else { before }.min(d.items.len());
    d.items.insert(at, it);
    let place = d.normalize_layers();
    place[at]
}

/// Reprend sur `target` les textes, images et boutons de navigation de `src`, dans l'ordre de superposition de `src`
/// (les vignettes de `target` restent telles quelles, a la place des vignettes de `src`).
/// Les boutons de navigation absents de `src` mais presents dans `target_default` (grille automatique de la page cible, par exemple
/// « Precedent » qui n'existe pas en page 1) sont ajoutes a leur place par defaut.
pub fn copy_decor(src: &PageDesign, target: PageDesign, target_default: &PageDesign) -> PageDesign {
    let target_name = target.name.clone();
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
    let mut d = PageDesign { items, layers: src.layers.clone(), bg: src.bg.clone(), name: target_name };
    d.normalize_layers();
    d
}

/// Ajoute une page libre vierge a la fin du menu principal et retourne son indice. La page precedente (automatique non
/// personnalisee : deja geree par la grille ; personnalisee ou libre : on ajoute un bouton « Suivant » s'il manque) mene a la nouvelle.
pub fn add_free_page(m: &mut ProjectModel) -> usize {
    if m.designs.generic {
        let k = m.designs.free.len();
        if let Some(prev) = m.designs.free.last_mut() {
            if !prev.items.iter().any(|i| matches!(&i.kind, Kind::Nav { action: NavAction::Next, .. })) {
                let front = prev.layers.last().map_or(0, |l| l.id);
                insert_item(prev, Item::nav(NavAction::Next, 1000, 1080 - 60 - 80, 360, 80), front);
            }
        }
        let mut d = PageDesign::blank_free(k);
        d.name = format!("Page {}", k + 1);
        m.designs.free.push(d);
        return k;
    }
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

// ------------------------------------------------------------------ simulation de la telecommande
/// Effet de la validation d'un bouton dans la simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimAction {
    /// Ouvre une page du menu.
    Go(Target),
    /// Lit une video depuis le debut.
    Play(usize),
    /// Lit une video depuis un chapitre : (video, chapitre).
    PlayChapter(usize, usize),
    /// Lit une video depuis un instant (vignette de chapitre d'une page generique) : (video, secondes).
    PlayFrom(usize, f64),
    /// Lit toutes les videos a la suite (« Tout lire » du menu principal), puis revient a l'accueil.
    PlayAllVideos,
    /// Page precedente / suivante du meme menu.
    Prev,
    Next,
    /// Retour au menu principal (page de la video).
    Back,
}

/// Un bouton de la page tel qu'il sera sur le disque : element de la mise en page, cadre (plan 1920), effet, voisins.
#[derive(Debug, Clone, PartialEq)]
pub struct SimButton {
    pub item: usize,
    pub rect: (i32, i32, i32, i32),
    pub action: SimAction,
    /// Page affichee a la fin de la lecture lancee par ce bouton (vignettes des pages generiques).
    pub ret: crate::design::SlotReturn,
    /// Sous-titres regles par le bouton.
    pub sub: crate::design::SubChoice,
    /// indices (dans la liste des boutons) des voisins (haut, bas, gauche, droite)
    pub neighbors: (usize, usize, usize, usize),
}

/// Boutons d'une page dans l'ordre du disque (le premier est selectionne a l'ouverture), avec la meme regle de voisinage que le
/// constructeur. `has_chapters(v)` : la video v a-t-elle un menu de chapitres ?
pub fn sim_buttons(info: &PageInfo, design: &PageDesign, style: &crate::style::ButtonStyle, n_videos: usize, has_chapters: &dyn Fn(usize) -> bool) -> Vec<SimButton> {
    let open_video = |v: usize| if has_chapters(v) { SimAction::Go(Target::Chapters(v, 0)) } else { SimAction::Play(v) };
    let asg = design.slot_assignment(info.items.len());
    let mut out: Vec<SimButton> = Vec::new();
    let mut e = 0;
    for (idx, it) in design.items.iter().enumerate() {
        let mut ret = crate::design::SlotReturn::Origin;
        let action = match &it.kind {
            Kind::Slot { video, link, start, ret: r, .. } => {
                ret = *r;
                let usual = match video {
                    Some(Some(v)) if *v < n_videos => Some(match start { Some(t) => SimAction::PlayFrom(*v, *t), None => open_video(*v) }),
                    Some(_) => None,
                    None => {
                        let a = asg.get(e).copied().unwrap_or(usize::MAX);
                        e += 1;
                        info.items.get(a).map(|&x| match info.video { Some(v) => SimAction::PlayChapter(v, x), None => open_video(x) })
                    }
                };
                match (usual, link) {
                    (None, _) => None,
                    (Some(_), Some(t)) => Some(SimAction::Go(*t)),
                    (Some(u), None) => Some(u),
                }
            }
            Kind::Nav { action, target, .. } if crate::design::nav_applicable(*action, info.menu, info.page, info.pages) => match action {
                NavAction::Prev => Some(SimAction::Prev),
                NavAction::Next => Some(SimAction::Next),
                NavAction::PlayAll => Some(info.video.map_or(SimAction::PlayAllVideos, SimAction::Play)),
                NavAction::Back => Some(SimAction::Back),
                NavAction::Goto => target.map(SimAction::Go),
            },
            _ => it.link().map(SimAction::Go),
        };
        let Some(action) = action else { continue };
        let st = it.look.style.as_ref().unwrap_or(style);
        let (mx, my) = st.margins(it.w, it.h);
        out.push(SimButton { item: idx, rect: (it.x - mx, it.y - my, it.w + 2 * mx, it.h + 2 * my), action, ret, sub: it.sub, neighbors: (0, 0, 0, 0) });
    }
    let rects: Vec<(i32, i32, i32, i32)> = out.iter().map(|b| b.rect).collect();
    for (b, n) in out.iter_mut().zip(crate::menu::auto_neighbors(&rects)) {
        b.neighbors = (n.0 as usize, n.1 as usize, n.2 as usize, n.3 as usize);
    }
    out
}

/// Page (dans `pages`, liste de `list_pages`) correspondant a une destination ; `None` si elle n'existe pas.
pub fn page_of_target(pages: &[PageInfo], t: Target) -> Option<PageRef> {
    match t {
        Target::Main(p) => pages.iter().find(|i| i.menu == MenuType::Main && i.page == p).map(|i| i.r),
        Target::Chapters(v, q) => pages.iter().find(|i| i.r == PageRef::Chapters(v, q)).map(|i| i.r),
        Target::Video(_) => None,
    }
}

/// Page du menu principal ou se trouve la video `v` (pages automatiques).
pub fn main_page_of_video(pages: &[PageInfo], v: usize) -> Option<PageRef> {
    pages.iter().find(|i| matches!(i.r, PageRef::Main(_)) && i.items.contains(&v)).map(|i| i.r)
}

// ------------------------------------------------------------------ pages generiques
/// Echange les pages `a` et `b` (pages generiques) ; les destinations, retours et l'accueil suivent leur page.
pub fn swap_pages(m: &mut ProjectModel, a: usize, b: usize) {
    let d = &mut m.designs;
    if a == b || a >= d.free.len() || b >= d.free.len() { return; }
    d.free.swap(a, b);
    let sw = |p: usize| if p == a { b } else if p == b { a } else { p };
    for pg in d.free.iter_mut() {
        for it in &mut pg.items {
            if let Some(Some(Target::Main(p))) = it.targets_mut() { *p = sw(*p); }
            if let Kind::Slot { ret: crate::design::SlotReturn::Page(p), .. } = &mut it.kind { *p = sw(*p); }
        }
    }
    if let Some(Target::Main(p)) = &mut d.home { *p = sw(*p); }
}

/// Deplace la page generique `from` a la place `to` (les autres glissent) ; destinations, retours et accueil suivent leur page.
pub fn move_page(m: &mut ProjectModel, from: usize, to: usize) {
    let d = &mut m.designs;
    let n = d.free.len();
    if from >= n || to >= n || from == to { return; }
    let pg = d.free.remove(from);
    d.free.insert(to, pg);
    // ancienne place -> nouvelle place
    let map = |p: usize| -> usize {
        if p == from { to }
        else if from < to && p > from && p <= to { p - 1 }
        else if to < from && p >= to && p < from { p + 1 }
        else { p }
    };
    for pg in d.free.iter_mut() {
        for it in &mut pg.items {
            if let Some(Some(Target::Main(p))) = it.targets_mut() { *p = map(*p); }
            if let Kind::Slot { ret: crate::design::SlotReturn::Page(p), .. } = &mut it.kind { *p = map(*p); }
        }
    }
    if let Some(Target::Main(p)) = &mut d.home { *p = map(*p); }
}

/// Met la page `to` en calques s'il lui en faut : ses elements actuels forment « Calque 1 ».
fn ensure_layered(d: &mut PageDesign) {
    if d.layers.is_empty() {
        d.layers.push(Layer { id: 1, name: "Calque 1".into(), ..Default::default() });
        for it in &mut d.items { it.layer = 1; }
    }
}

/// Deplace (ou copie) l'element `i` de la page generique `from` vers la page `to`, au premier plan de son calque du dessus.
/// Retourne son indice dans la page `to`.
pub fn transfer_item(d: &mut Designs, from: usize, i: usize, to: usize, copy: bool) -> Option<usize> {
    if from == to || to >= d.free.len() || i >= d.free.get(from)?.items.len() { return None; }
    let it = if copy { d.free[from].items[i].clone() } else { d.free[from].items.remove(i) };
    let dst = &mut d.free[to];
    let top = dst.layers.last().map_or(0, |l| l.id);
    Some(insert_item(dst, it, top))
}

/// Deplace (ou copie) le calque `layer` de la page `from`, avec ses elements, au premier plan de la page `to` (meme nom, nouvel
/// identifiant). Retourne son identifiant dans la page `to`.
pub fn transfer_layer(d: &mut Designs, from: usize, layer: u32, to: usize, copy: bool) -> Option<u32> {
    if from == to || to >= d.free.len() { return None; }
    let src = d.free.get(from)?;
    let l = src.layers.iter().find(|l| l.id == layer)?.clone();
    let items: Vec<Item> = src.items.iter().filter(|it| it.layer == layer).cloned().collect();
    if !copy {
        let s = &mut d.free[from];
        s.items.retain(|it| it.layer != layer);
        s.layers.retain(|x| x.id != layer);
        s.normalize_layers();
    }
    let dst = &mut d.free[to];
    ensure_layered(dst);
    let id = dst.layers.iter().map(|x| x.id).max().unwrap_or(0) + 1;
    dst.layers.push(Layer { id, ..l });
    for mut it in items {
        it.layer = id;
        dst.items.push(it);
    }
    dst.normalize_layers();
    Some(id)
}

/// Chapitres de chaque video pour la conversion (instants en secondes, noms) ; `Err` tant qu'une duree utile n'est pas connue
/// (chapitres automatiques).
fn gen_videos(m: &ProjectModel, durations: &HashMap<String, f64>) -> Result<Vec<crate::convert::GenVideo>, String> {
    let mut out = Vec::new();
    for (i, v) in m.videos.iter().enumerate() {
        let d = durations.get(&v.file).copied();
        if v.chapters.is_empty() && v.auto_s.is_some() && d.is_none() {
            return Err(format!("duree de la video {} inconnue", i + 1));
        }
        out.push(crate::convert::GenVideo { title: video_title(m, i), chapters: v.chapter_list(d) });
    }
    Ok(out)
}

/// Convertit un ancien projet (pages automatiques, pages libres, menus de chapitres) en pages generiques.
/// `Err` si une duree necessaire n'est pas encore connue (reessayer plus tard).
pub fn convert_to_generic(m: &mut ProjectModel, durations: &HashMap<String, f64>) -> Result<(), String> {
    if m.designs.generic { return Ok(()); }
    let gv = gen_videos(m, durations)?;
    m.designs = crate::convert::generate_pages(&gv, m.par_page, &m.title, &m.designs)?;
    Ok(())
}

/// Ajoute a la fin les pages d'un menu complet genere automatiquement (grille de toutes les videos, menus de chapitres).
/// Retourne le nombre de pages ajoutees.
pub fn append_generated_pages(m: &mut ProjectModel, durations: &HashMap<String, f64>) -> Result<usize, String> {
    convert_to_generic(m, durations)?;
    let gv = gen_videos(m, durations)?;
    let gen = crate::convert::generate_pages(&gv, m.par_page, &m.title, &Designs::default())?;
    let off = m.designs.free.len();
    let mut pages = gen.free;
    for pg in &mut pages {
        for it in &mut pg.items {
            if let Some(Some(Target::Main(p))) = it.targets_mut() { *p += off; }
            if let Kind::Slot { ret: crate::design::SlotReturn::Page(p), .. } = &mut it.kind { *p += off; }
        }
    }
    let n = pages.len();
    m.designs.free.extend(pages);
    Ok(n)
}
