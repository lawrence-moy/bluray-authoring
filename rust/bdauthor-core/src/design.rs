//! Mise en page personnalisee des pages de menu : vignettes, boutons de navigation, textes et images placees librement.
//!
//! Toutes les coordonnees sont exprimees sur un plan de reference 1920x1080 ; le constructeur les met a l'echelle de la taille du menu.
//! Sans mise en page personnalisee, une page garde la grille automatique (`default_main` / `default_chapter`), identique a celle de Python.
use crate::ig::py_round;
use crate::json::Value;
use crate::project::{layout, MAX_PER_PAGE};
use std::collections::BTreeMap;

pub const PLANE: (i32, i32) = (1920, 1080);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavAction { Prev, Next, PlayAll, Back }

impl NavAction {
    pub const ALL: [NavAction; 4] = [NavAction::Prev, NavAction::Next, NavAction::PlayAll, NavAction::Back];
    pub fn name(self) -> &'static str {
        match self { NavAction::Prev => "precedent", NavAction::Next => "suivant", NavAction::PlayAll => "tout_lire", NavAction::Back => "retour" }
    }
    pub fn from_name(s: &str) -> Option<Self> { Self::ALL.into_iter().find(|a| a.name() == s) }
    pub fn default_text(self) -> &'static str {
        match self { NavAction::Prev => "< Précédent", NavAction::Next => "Suivant >", NavAction::PlayAll => "Tout lire", NavAction::Back => "Retour" }
    }
    pub fn label(self) -> &'static str {
        match self { NavAction::Prev => "Page précédente", NavAction::Next => "Page suivante", NavAction::PlayAll => "Tout lire", NavAction::Back => "Retour" }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align { Left, Center, Right }
impl Align {
    pub fn name(self) -> &'static str { match self { Align::Left => "gauche", Align::Center => "centre", Align::Right => "droite" } }
    pub fn from_name(s: &str) -> Option<Self> { [Align::Left, Align::Center, Align::Right].into_iter().find(|a| a.name() == s) }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// Vignette cliquable + libelle ; `thumb_h` = hauteur de la vignette (le reste de `h` porte le libelle). Les vignettes se remplissent dans l'ordre de la liste.
    /// `assign` : element de la page montre par cette vignette (0-based) ; `None` = automatique (voir `PageDesign::slot_assignment`).
    Slot { thumb_h: i32, assign: Option<usize> },
    Nav { action: NavAction, text: String },
    /// `font` : vide = police par defaut ; sinon fichier .ttf/.otf (chemin absolu ou relatif au projet) ou nom d'un fichier de police installe.
    Text { text: String, size: i32, color: [u8; 3], align: Align, font: String },
    Image { path: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item { pub x: i32, pub y: i32, pub w: i32, pub h: i32, pub kind: Kind }

impl Item {
    pub fn is_button(&self) -> bool { matches!(self.kind, Kind::Slot { .. } | Kind::Nav { .. }) }
    pub fn is_decor(&self) -> bool { !self.is_button() }
    /// (x, y, largeur, hauteur) mis a l'echelle `s` (arrondi comme Python).
    pub fn scaled(&self, s: f64) -> (i32, i32, i32, i32) {
        let r = |v: i32| py_round(v as f64 * s) as i32;
        (r(self.x), r(self.y), r(self.w), r(self.h))
    }
    pub fn text(text: &str, x: i32, y: i32, w: i32, h: i32, size: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Text { text: text.into(), size, color: [255, 255, 255], align: Align::Center, font: String::new() } }
    }
    pub fn nav(action: NavAction, x: i32, y: i32, w: i32, h: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Nav { action, text: action.default_text().into() } }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuType { Main, Chapters }

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageDesign { pub items: Vec<Item> }

#[derive(Debug, Clone, PartialEq)]
pub struct Problem { pub error: bool, pub msg: String }

/// Ce bouton de navigation existe-t-il sur cette page ?
pub fn nav_applicable(action: NavAction, menu: MenuType, page: usize, pages: usize) -> bool {
    match action {
        NavAction::Prev => page > 0,
        NavAction::Next => page + 1 < pages,
        NavAction::PlayAll | NavAction::Back => menu == MenuType::Chapters,
    }
}

const NAV_Y: i32 = 1080 - 60 - 80;

impl PageDesign {
    pub fn slots(&self) -> Vec<&Item> { self.items.iter().filter(|i| matches!(i.kind, Kind::Slot { .. })).collect() }
    pub fn slot_count(&self) -> usize { self.slots().len() }

    /// Pour chaque vignette (dans l'ordre de la liste), l'indice (0-based) de l'element de la page qu'elle montre.
    /// Les affectations explicites valides (dans la page, sans doublon) sont respectees ; les autres vignettes recoivent,
    /// dans l'ordre de la liste, les elements restants dans l'ordre. `count` = nombre d'elements de la page.
    pub fn slot_assignment(&self, count: usize) -> Vec<usize> {
        let slots = self.slots();
        let mut out = vec![usize::MAX; slots.len()];
        let mut used = vec![false; count];
        for (k, s) in slots.iter().enumerate() {
            if let Kind::Slot { assign: Some(a), .. } = s.kind {
                if a < count && !used[a] { used[a] = true; out[k] = a; }
            }
        }
        let mut free = (0..count).filter(|e| !used[*e]);
        for o in out.iter_mut().filter(|o| **o == usize::MAX) {
            *o = free.next().unwrap_or(0);
        }
        out
    }

    /// Fait montrer a la vignette `item` (indice dans `items`) l'element `element` de la page (`None` : retour a l'automatique).
    /// Si une autre vignette montrait deja cet element, les deux echangent leur contenu.
    pub fn assign_slot(&mut self, item: usize, element: Option<usize>, count: usize) {
        let Some(e) = element else {
            if let Some(Kind::Slot { assign, .. }) = self.items.get_mut(item).map(|i| &mut i.kind) { *assign = None; }
            return;
        };
        let pos: Vec<usize> = self.items.iter().enumerate().filter(|(_, i)| matches!(i.kind, Kind::Slot { .. })).map(|(k, _)| k).collect();
        let Some(k) = pos.iter().position(|&p| p == item) else { return };
        let eff = self.slot_assignment(count);
        if e >= count { return; }
        let prev = eff[k];
        let other = eff.iter().position(|&x| x == e);
        if let Kind::Slot { assign, .. } = &mut self.items[item].kind { *assign = Some(e); }
        if let Some(t) = other.filter(|&t| t != k) {
            if let Kind::Slot { assign, .. } = &mut self.items[pos[t]].kind { *assign = Some(prev); }
        }
    }

    fn grid(count: usize, reserve: i32) -> Result<Vec<Item>, String> {
        Ok(layout(count, (1920, 1080), reserve).map_err(|e| e.to_string())?
            .into_iter().map(|c| Item { x: c.x, y: c.y, w: c.w, h: c.h, kind: Kind::Slot { thumb_h: c.th, assign: None } }).collect())
    }

    /// Grille automatique du menu principal (page `page` sur `pages`) pour `count` videos.
    pub fn default_main(count: usize, page: usize, pages: usize) -> Result<Self, String> {
        let mut items = Self::grid(count, if pages > 1 { 140 } else { 0 })?;
        if page > 0 { items.push(Item::nav(NavAction::Prev, 560, NAV_Y, 360, 80)); }
        if page + 1 < pages { items.push(Item::nav(NavAction::Next, 1000, NAV_Y, 360, 80)); }
        Ok(PageDesign { items })
    }

    /// Grille automatique d'une page de menu de chapitres.
    pub fn default_chapter(count: usize, page: usize, pages: usize) -> Result<Self, String> {
        let mut items = Self::grid(count, 140)?;
        if pages == 1 {
            items.push(Item::nav(NavAction::PlayAll, 560, NAV_Y, 360, 80));
            items.push(Item::nav(NavAction::Back, 1000, NAV_Y, 360, 80));
        } else {
            if page > 0 { items.push(Item::nav(NavAction::Prev, 260, NAV_Y, 320, 80)); }
            items.push(Item::nav(NavAction::PlayAll, 620, NAV_Y, 320, 80));
            items.push(Item::nav(NavAction::Back, 980, NAV_Y, 320, 80));
            if page + 1 < pages { items.push(Item::nav(NavAction::Next, 1340, NAV_Y, 320, 80)); }
        }
        Ok(PageDesign { items })
    }

    /// Point de depart de l'editeur : la grille automatique + un texte de titre.
    pub fn starting_point(menu: MenuType, count: usize, page: usize, pages: usize, title: &str) -> Result<Self, String> {
        let mut d = match menu { MenuType::Main => Self::default_main(count, page, pages)?, MenuType::Chapters => Self::default_chapter(count, page, pages)? };
        d.items.insert(0, Item::text(title, 100, 90, 1720, 110, 84));
        Ok(d)
    }

    /// Verifie la page ; `expected_slots` = nombre d'elements (videos ou chapitres) a placer.
    pub fn validate(&self, expected_slots: usize, menu: MenuType, page: usize, pages: usize) -> Vec<Problem> {
        let mut p = Vec::new();
        let (err, warn) = (|m: String| Problem { error: true, msg: m }, |m: String| Problem { error: false, msg: m });
        let n = self.slot_count();
        if n != expected_slots {
            p.push(err(format!("{n} vignette(s) placée(s) pour {expected_slots} élément(s) sur cette page.")));
        }
        if n > MAX_PER_PAGE { p.push(err(format!("{n} vignettes : {MAX_PER_PAGE} au maximum par page."))); }
        for (k, it) in self.items.iter().enumerate() {
            if it.w < 8 || it.h < 8 { p.push(err(format!("Élément {} trop petit.", k + 1))); }
            if it.x < 0 || it.y < 0 || it.x + it.w > PLANE.0 || it.y + it.h > PLANE.1 {
                let m = format!("Élément {} dépasse de l'écran.", k + 1);
                p.push(if it.is_button() { err(m) } else { warn(m) });
            }
            if let Kind::Slot { thumb_h, .. } = it.kind {
                if thumb_h < 8 || thumb_h > it.h { p.push(err(format!("Élément {} : hauteur de vignette incohérente.", k + 1))); }
            }
            if let Kind::Nav { action, .. } = it.kind {
                if !nav_applicable(action, menu, page, pages) { p.push(warn(format!("« {} » n'apparaîtra pas sur cette page.", action.label()))); }
            }
        }
        let active: Vec<(usize, &Item)> = self.items.iter().enumerate().filter(|(_, i)| match &i.kind {
            Kind::Slot { .. } => true,
            Kind::Nav { action, .. } => nav_applicable(*action, menu, page, pages),
            _ => false,
        }).collect();
        for (ai, (a, ia)) in active.iter().enumerate() {
            for (b, ib) in &active[ai + 1..] {
                if ia.x < ib.x + ib.w && ib.x < ia.x + ia.w && ia.y < ib.y + ib.h && ib.y < ia.y + ia.h {
                    p.push(warn(format!("Les éléments {} et {} se chevauchent.", a + 1, b + 1)));
                }
            }
        }
        p
    }

    // ---------------------------------------------------------------- JSON
    pub fn to_value(&self) -> Value {
        let num = |v: i32| Value::Num(v as f64);
        let items = self.items.iter().map(|it| {
            let mut o: Vec<(String, Value)> = Vec::new();
            let mut put = |k: &str, v: Value| o.push((k.to_string(), v));
            match &it.kind {
                Kind::Slot { thumb_h, assign } => {
                    put("type", Value::Str("vignette".into())); put("vignette_h", num(*thumb_h));
                    if let Some(a) = assign { put("element", num(*a as i32 + 1)); }
                }
                Kind::Nav { action, text } => { put("type", Value::Str("navigation".into())); put("action", Value::Str(action.name().into())); put("texte", Value::Str(text.clone())); }
                Kind::Text { text, size, color, align, font } => {
                    if !font.is_empty() { put("police", Value::Str(font.clone())); }
                    put("type", Value::Str("texte".into())); put("texte", Value::Str(text.clone())); put("taille", num(*size));
                    put("couleur", Value::Str(format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]))); put("alignement", Value::Str(align.name().into()));
                }
                Kind::Image { path } => { put("type", Value::Str("image".into())); put("fichier", Value::Str(path.clone())); }
            }
            put("x", num(it.x)); put("y", num(it.y)); put("w", num(it.w)); put("h", num(it.h));
            Value::Obj(o)
        }).collect();
        Value::Obj(vec![("elements".into(), Value::Arr(items))])
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let mut items = Vec::new();
        for (k, e) in v.get("elements").and_then(Value::as_array).ok_or("page sans « elements »")?.iter().enumerate() {
            let at = |key: &str| -> Result<i32, String> { e.get(key).and_then(Value::as_f64).map(|x| x.round() as i32).ok_or_else(|| format!("élément {} : « {key} » manquant", k + 1)) };
            let st = |key: &str| e.get(key).and_then(Value::as_str);
            let kind = match st("type").ok_or_else(|| format!("élément {} sans type", k + 1))? {
                "vignette" => Kind::Slot { thumb_h: at("vignette_h")?, assign: e.get("element").and_then(Value::as_f64).map(|x| (x.round() as i64 - 1).max(0) as usize) },
                "navigation" => {
                    let action = st("action").and_then(NavAction::from_name).ok_or_else(|| format!("élément {} : action inconnue", k + 1))?;
                    Kind::Nav { action, text: st("texte").unwrap_or(action.default_text()).to_string() }
                }
                "texte" => {
                    let c = st("couleur").unwrap_or("#ffffff").trim_start_matches('#');
                    let rgb = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| format!("élément {} : couleur invalide", k + 1))?;
                    Kind::Text { text: st("texte").unwrap_or("").to_string(), size: at("taille")?, color: [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8],
                                 align: st("alignement").and_then(Align::from_name).unwrap_or(Align::Center), font: st("police").unwrap_or("").to_string() }
                }
                "image" => Kind::Image { path: st("fichier").ok_or_else(|| format!("élément {} : « fichier » manquant", k + 1))?.to_string() },
                t => return Err(format!("élément {} : type « {t} » inconnu", k + 1)),
            };
            items.push(Item { x: at("x")?, y: at("y")?, w: at("w")?, h: at("h")?, kind });
        }
        Ok(PageDesign { items })
    }
}

/// Mises en page personnalisees du projet. Une page absente garde la grille automatique.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Designs {
    /// page du menu principal (0-based) -> mise en page
    pub main: BTreeMap<usize, PageDesign>,
    /// (video 0-based, page 0-based) -> mise en page du menu de chapitres
    pub chapters: BTreeMap<(usize, usize), PageDesign>,
}

impl Designs {
    pub fn is_empty(&self) -> bool { self.main.is_empty() && self.chapters.is_empty() }

    /// Echange deux videos (les menus de chapitres suivent leur video).
    pub fn swap_videos(&mut self, a: usize, b: usize) {
        let old = std::mem::take(&mut self.chapters);
        self.chapters = old.into_iter().map(|((v, p), d)| (((if v == a { b } else if v == b { a } else { v }), p), d)).collect();
    }

    /// Retire une video : ses menus de chapitres disparaissent, les suivants sont renumerotes.
    pub fn remove_video(&mut self, i: usize) {
        let old = std::mem::take(&mut self.chapters);
        self.chapters = old.into_iter().filter(|((v, _), _)| *v != i).map(|((v, p), d)| ((if v > i { v - 1 } else { v }, p), d)).collect();
    }

    pub fn to_value(&self) -> Value {
        let mut main = Vec::new();
        for (p, d) in &self.main { main.push(((p + 1).to_string(), d.to_value())); }
        let mut by_video: BTreeMap<usize, Vec<(String, Value)>> = BTreeMap::new();
        for ((v, p), d) in &self.chapters { by_video.entry(*v).or_default().push(((p + 1).to_string(), d.to_value())); }
        let ch = by_video.into_iter().map(|(v, pages)| ((v + 1).to_string(), Value::Obj(pages))).collect();
        Value::Obj(vec![("principal".into(), Value::Obj(main)), ("chapitres".into(), Value::Obj(ch))])
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let mut d = Designs::default();
        let idx = |k: &str| k.parse::<usize>().ok().filter(|n| *n >= 1).map(|n| n - 1).ok_or_else(|| format!("numéro de page ou de vidéo invalide : « {k} »"));
        if let Some(Value::Obj(m)) = v.get("principal") {
            for (k, page) in m { d.main.insert(idx(k)?, PageDesign::from_value(page).map_err(|e| format!("menu principal, page {k} : {e}"))?); }
        }
        if let Some(Value::Obj(m)) = v.get("chapitres") {
            for (vk, pages) in m {
                let Value::Obj(pp) = pages else { return Err(format!("chapitres, vidéo {vk} : objet attendu")) };
                for (pk, page) in pp { d.chapters.insert((idx(vk)?, idx(pk)?), PageDesign::from_value(page).map_err(|e| format!("chapitres vidéo {vk}, page {pk} : {e}"))?); }
            }
        }
        Ok(d)
    }
}
