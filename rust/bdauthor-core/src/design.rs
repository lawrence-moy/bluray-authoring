//! Mise en page personnalisee des pages de menu : vignettes, boutons de navigation, textes et images placees librement.
//!
//! Toutes les coordonnees sont exprimees sur un plan de reference 1920x1080 ; le constructeur les met a l'echelle de la taille du menu.
//! Sans mise en page personnalisee, une page garde la grille automatique (`default_main` / `default_chapter`), identique a celle de Python.
use crate::ig::py_round;
use crate::json::Value;
use crate::style::{ButtonStyle, Look, Skin, Weight};
use crate::project::{layout, MAX_PER_PAGE};
use std::collections::BTreeMap;

pub const PLANE: (i32, i32) = (1920, 1080);

/// Destination d'un bouton « Aller a », ou d'un texte / d'une image cliquable : une page du menu ou une video a lire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Page du menu principal (0-based, pages automatiques puis pages libres).
    Main(usize),
    /// Page de chapitres : (video, page), 0-based.
    Chapters(usize, usize),
    /// Lecture d'une video (0-based) depuis le debut.
    Video(usize),
}

impl Target {
    /// Valeur JSON (champ « cible »).
    pub fn to_value(self) -> Value {
        let num = |v: usize| Value::Num((v + 1) as f64);
        let o = match self {
            Target::Main(p) => vec![("menu".to_string(), Value::Str("principal".into())), ("page".to_string(), num(p))],
            Target::Chapters(v, p) => vec![("menu".to_string(), Value::Str("chapitres".into())), ("video".to_string(), num(v)), ("page".to_string(), num(p))],
            Target::Video(v) => vec![("lire".to_string(), Value::Str("video".into())), ("video".to_string(), num(v))],
        };
        Value::Obj(o)
    }
    pub fn from_value(c: &Value) -> Option<Target> {
        let n = |key: &str| c.get(key).and_then(Value::as_f64).map(|x| (x.round() as i64 - 1).max(0) as usize);
        if c.get("lire").and_then(Value::as_str) == Some("video") {
            return n("video").map(Target::Video);
        }
        match c.get("menu").and_then(Value::as_str) {
            Some("principal") => n("page").map(Target::Main),
            Some("chapitres") => n("video").zip(n("page")).map(|(v, p)| Target::Chapters(v, p)),
            _ => None,
        }
    }
    /// Renumerotation quand deux videos echangent leur place.
    fn swap_videos(&mut self, a: usize, b: usize) {
        let sw = |v: &mut usize| *v = if *v == a { b } else if *v == b { a } else { *v };
        match self { Target::Chapters(v, _) | Target::Video(v) => sw(v), Target::Main(_) => {} }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavAction { Prev, Next, PlayAll, Back, Goto }

impl NavAction {
    pub const ALL: [NavAction; 5] = [NavAction::Prev, NavAction::Next, NavAction::PlayAll, NavAction::Back, NavAction::Goto];
    pub fn name(self) -> &'static str {
        match self { NavAction::Prev => "precedent", NavAction::Next => "suivant", NavAction::PlayAll => "tout_lire", NavAction::Back => "retour", NavAction::Goto => "aller_a" }
    }
    pub fn from_name(s: &str) -> Option<Self> { Self::ALL.into_iter().find(|a| a.name() == s) }
    pub fn default_text(self) -> &'static str {
        match self { NavAction::Prev => "< Précédent", NavAction::Next => "Suivant >", NavAction::PlayAll => "Tout lire", NavAction::Back => "Retour", NavAction::Goto => "Aller à…" }
    }
    pub fn label(self) -> &'static str {
        match self { NavAction::Prev => "Page précédente", NavAction::Next => "Page suivante", NavAction::PlayAll => "Tout lire", NavAction::Back => "Retour", NavAction::Goto => "Aller à une page ou lire une vidéo" }
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
    /// `video` : `None` = vignette d'un element de la page (voir `assign`). `Some(_)` = vignette d'une video choisie, quelle que soit
    /// la page (`assign` ignore) : `Some(Some(v))` montre la video v du projet (0-based), `Some(None)` n'a pas (ou plus) de video et
    /// n'apparait pas sur le disque. Toutes les vignettes des pages libres sont de ce type. Une meme video peut etre sur plusieurs vignettes.
    /// `link` : destination au clic ; `None` = comportement habituel (video : son menu de chapitres s'il existe, sinon la lecture ;
    /// chapitre : lecture depuis ce chapitre).
    Slot { thumb_h: i32, assign: Option<usize>, video: Option<Option<usize>>, link: Option<Target> },
    /// `target` : page visee par une action `Goto` (ignore pour les autres actions).
    Nav { action: NavAction, text: String, target: Option<Target> },
    /// `font` : vide = police par defaut ; sinon fichier .ttf/.otf (chemin absolu ou relatif au projet) ou nom d'un fichier de police installe.
    /// `link` : si present, le texte devient un bouton qui mene a cette destination.
    Text { text: String, size: i32, color: [u8; 3], align: Align, font: String, link: Option<Target> },
    /// `link` : si present, l'image devient un bouton qui mene a cette destination.
    Image { path: String, link: Option<Target> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub kind: Kind,
    /// Apparence propre a ce bouton (style, images d'etats) ; par defaut celle du projet.
    pub look: Look,
}

impl Item {
    pub fn is_button(&self) -> bool { matches!(self.kind, Kind::Slot { .. } | Kind::Nav { .. }) || self.link().is_some() }
    /// Destination d'un texte ou d'une image cliquable.
    pub fn link(&self) -> Option<Target> {
        match self.kind { Kind::Text { link, .. } | Kind::Image { link, .. } | Kind::Slot { link, .. } => link, _ => None }
    }
    /// Vignette d'un element de la page (et non d'une video choisie) ?
    pub fn is_element_slot(&self) -> bool { matches!(self.kind, Kind::Slot { video: None, .. }) }
    /// Vignette d'une video choisie (meme sans video pour l'instant) ?
    pub fn is_video_slot(&self) -> bool { matches!(self.kind, Kind::Slot { video: Some(_), .. }) }
    /// Vignette montrant une video choisie : son indice.
    pub fn fixed_video(&self) -> Option<usize> { if let Kind::Slot { video: Some(v), .. } = self.kind { v } else { None } }
    /// Vignette vierge (elements de la page dans l'ordre, comportement habituel).
    pub fn slot(x: i32, y: i32, w: i32, h: i32, thumb_h: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Slot { thumb_h, assign: None, video: None, link: None }, look: Look::default() }
    }
    /// Destinations modifiables de l'element : cible d'un bouton « Aller a », lien d'un texte ou d'une image.
    pub fn targets_mut(&mut self) -> Option<&mut Option<Target>> {
        match &mut self.kind {
            Kind::Nav { action: NavAction::Goto, target, .. } => Some(target),
            Kind::Text { link, .. } | Kind::Image { link, .. } | Kind::Slot { link, .. } => Some(link),
            _ => None,
        }
    }
    pub fn is_decor(&self) -> bool { !self.is_button() }
    /// (x, y, largeur, hauteur) mis a l'echelle `s` (arrondi comme Python).
    pub fn scaled(&self, s: f64) -> (i32, i32, i32, i32) {
        let r = |v: i32| py_round(v as f64 * s) as i32;
        (r(self.x), r(self.y), r(self.w), r(self.h))
    }
    pub fn text(text: &str, x: i32, y: i32, w: i32, h: i32, size: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Text { text: text.into(), size, color: [255, 255, 255], align: Align::Center, font: String::new(), link: None }, look: Look::default() }
    }
    pub fn nav(action: NavAction, x: i32, y: i32, w: i32, h: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Nav { action, text: action.default_text().into(), target: if action == NavAction::Goto { Some(Target::Main(0)) } else { None } }, look: Look::default() }
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
        NavAction::Goto => true,
    }
}

const NAV_Y: i32 = 1080 - 60 - 80;

impl PageDesign {
    /// Vignettes des elements de la page (hors vignettes de videos choisies), dans l'ordre de la liste.
    pub fn slots(&self) -> Vec<&Item> { self.items.iter().filter(|i| i.is_element_slot()).collect() }
    pub fn slot_count(&self) -> usize { self.slots().len() }
    /// Toutes les vignettes, y compris celles de videos choisies.
    pub fn all_slot_count(&self) -> usize { self.items.iter().filter(|i| matches!(i.kind, Kind::Slot { .. })).count() }

    /// Pour chaque vignette d'element (dans l'ordre de la liste), l'indice (0-based) de l'element de la page qu'elle montre,
    /// ou `usize::MAX` si elle n'a rien a montrer. Les affectations explicites valides sont respectees (plusieurs vignettes peuvent
    /// montrer le meme element) ; les vignettes automatiques recoivent, dans l'ordre de la liste, les elements que personne ne
    /// montre encore. `count` = nombre d'elements de la page.
    pub fn slot_assignment(&self, count: usize) -> Vec<usize> {
        let slots = self.slots();
        let mut out = vec![usize::MAX; slots.len()];
        let mut used = vec![false; count];
        for (k, s) in slots.iter().enumerate() {
            if let Kind::Slot { assign: Some(a), .. } = s.kind {
                if a < count { used[a] = true; out[k] = a; }
            }
        }
        let mut free = (0..count).filter(|e| !used[*e]);
        for (k, s) in slots.iter().enumerate() {
            let explicit = matches!(s.kind, Kind::Slot { assign: Some(a), .. } if a < count);
            if !explicit { out[k] = free.next().unwrap_or(usize::MAX); }
        }
        out
    }

    /// Elements de la page qu'aucune vignette ne montre.
    pub fn unshown_elements(&self, count: usize) -> Vec<usize> {
        let a = self.slot_assignment(count);
        (0..count).filter(|e| !a.contains(e)).collect()
    }

    /// Fait montrer a la vignette `item` (indice dans `items`) l'element `element` de la page (`None` : retour a l'automatique).
    /// Un element peut etre montre par plusieurs vignettes.
    pub fn assign_slot(&mut self, item: usize, element: Option<usize>, count: usize) {
        if element.map_or(false, |e| e >= count) { return; }
        if let Some(Kind::Slot { assign, video, .. }) = self.items.get_mut(item).map(|i| &mut i.kind) {
            *video = None;
            *assign = element;
        }
    }

    fn grid(count: usize, reserve: i32) -> Result<Vec<Item>, String> {
        Ok(layout(count, (1920, 1080), reserve).map_err(|e| e.to_string())?
            .into_iter().map(|c| Item::slot(c.x, c.y, c.w, c.h, c.th)).collect())
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
        let empty = self.slot_assignment(expected_slots).iter().filter(|&&a| a == usize::MAX).count();
        if empty > 0 {
            p.push(err(format!("{empty} vignette(s) sans élément à montrer : choisissez leur contenu ou supprimez-les.")));
        }
        let missing = self.unshown_elements(expected_slots);
        if !missing.is_empty() {
            let l: Vec<String> = missing.iter().map(|e| (e + 1).to_string()).collect();
            p.push(warn(format!("Élément(s) {} de la page sans vignette : ils ne seront pas accessibles depuis cette page.", l.join(", "))));
        }
        let all = self.all_slot_count();
        if all > MAX_PER_PAGE { p.push(err(format!("{all} vignettes : {MAX_PER_PAGE} au maximum par page."))); }
        for (k, it) in self.items.iter().enumerate() {
            if it.w < 8 || it.h < 8 { p.push(err(format!("Élément {} trop petit.", k + 1))); }
            if it.x < 0 || it.y < 0 || it.x + it.w > PLANE.0 || it.y + it.h > PLANE.1 {
                let m = format!("Élément {} dépasse de l'écran.", k + 1);
                p.push(if it.is_button() { err(m) } else { warn(m) });
            }
            if let Kind::Slot { thumb_h, video, .. } = it.kind {
                if thumb_h < 8 || thumb_h > it.h { p.push(err(format!("Élément {} : hauteur de vignette incohérente.", k + 1))); }
                if video == Some(None) { p.push(warn(format!("Élément {} : vignette sans vidéo, elle n'apparaîtra pas sur le disque.", k + 1))); }
            }
            if let Kind::Nav { action: NavAction::Goto, target: None, .. } = it.kind {
                p.push(err(format!("Élément {} : choisissez la page cible du bouton.", k + 1)));
            }
            if let Kind::Nav { action, .. } = it.kind {
                if !nav_applicable(action, menu, page, pages) { p.push(warn(format!("« {} » n'apparaîtra pas sur cette page.", action.label()))); }
            }
        }
        let active: Vec<(usize, &Item)> = self.items.iter().enumerate().filter(|(_, i)| match &i.kind {
            Kind::Slot { .. } => true,
            Kind::Nav { action, .. } => nav_applicable(*action, menu, page, pages),
            _ => i.link().is_some(),
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

    /// Page libre : page `page` (0-based) sur `pages` du menu principal, `n_videos` videos dans le projet. Chaque vignette doit
    /// montrer une video existante (`assign`) ; les doublons sont permis.
    pub fn validate_free(&self, page: usize, pages: usize, n_videos: usize) -> Vec<Problem> {
        let mut p: Vec<Problem> = self.validate(self.slot_count(), MenuType::Main, page, pages);
        let mut k = 0;
        for it in &self.items {
            if let Kind::Slot { video, .. } = it.kind {
                k += 1;
                match video {
                    None => p.push(Problem { error: true, msg: format!("Vignette {k} : choisissez une vidéo.") }),
                    Some(None) => {} // signale par `validate`
                    Some(Some(v)) if v >= n_videos => p.push(Problem { error: true, msg: format!("Vignette {k} : la vidéo {} n'existe pas.", v + 1) }),
                    _ => {}
                }
            }
        }
        if !self.items.iter().any(|i| matches!(i.kind, Kind::Nav { action: NavAction::Prev, .. })) && page > 0 {
            p.push(Problem { error: false, msg: "Aucun bouton « Précédent » : cette page serait une impasse.".into() });
        }
        p
    }

    /// Verifie les pages visees par les boutons « Aller a » ; `exists` dit si une page existe dans le projet.
    pub fn check_targets(&self, exists: &dyn Fn(Target) -> bool) -> Vec<Problem> {
        let mut p = Vec::new();
        for (k, it) in self.items.iter().enumerate() {
            let t = match it.kind { Kind::Nav { action: NavAction::Goto, target: Some(t), .. } => Some(t), _ => it.link() };
            if let Some(t) = t {
                if !exists(t) { p.push(Problem { error: true, msg: format!("Élément {} : la destination n'existe pas.", k + 1) }); }
            }
            if let Some(v) = it.fixed_video() {
                if !exists(Target::Video(v)) { p.push(Problem { error: true, msg: format!("Élément {} : la vidéo {} n'existe pas.", k + 1, v + 1) }); }
            }
        }
        p
    }

    /// Poids estime de la page : images de boutons a decoder et delai d'affichage. `count` = elements de la page, `base` = style du
    /// projet, `plane` = taille du menu en pixels.
    pub fn weight(&self, count: usize, base: &ButtonStyle, menu: MenuType, page: usize, pages: usize, plane: (u32, u32)) -> Weight {
        let k = plane.0 as f64 / PLANE.0 as f64;
        let asg = self.slot_assignment(count);
        let (mut bytes, mut buttons, mut e) = (0u64, 0usize, 0usize);
        for it in &self.items {
            let shown = match &it.kind {
                Kind::Slot { video: None, .. } => { let a = asg.get(e).copied().unwrap_or(usize::MAX); e += 1; a != usize::MAX }
                Kind::Slot { video: Some(v), .. } => v.is_some(),
                Kind::Nav { action, .. } => nav_applicable(*action, menu, page, pages),
                _ => it.link().is_some(),
            };
            if !shown { continue; }
            let st = it.look.style.as_ref().unwrap_or(base).scaled(k);
            let (_, _, w, h) = it.scaled(k);
            let (mx, my) = st.margins(w, h);
            let states = if !it.look.skin.activated.is_empty() { 3 } else { st.state_count() };
            bytes += ((w + 2 * mx).max(1) as u64) * ((h + 2 * my).max(1) as u64) * states;
            buttons += 1;
        }
        let plane_px = plane.0 as u64 * plane.1 as u64;
        Weight { bytes, seconds: (plane_px + bytes) as f64 / crate::style::DECODE_RATE, buttons }
    }

    /// Page libre vierge : seulement le bouton « Précédent ».
    pub fn blank_free(page: usize) -> Self {
        let mut items = Vec::new();
        if page > 0 { items.push(Item::nav(NavAction::Prev, 560, NAV_Y, 360, 80)); }
        PageDesign { items }
    }

    // ---------------------------------------------------------------- JSON
    pub fn to_value(&self) -> Value {
        let num = |v: i32| Value::Num(v as f64);
        let items = self.items.iter().map(|it| {
            let mut o: Vec<(String, Value)> = Vec::new();
            let mut put = |k: &str, v: Value| o.push((k.to_string(), v));
            match &it.kind {
                Kind::Slot { thumb_h, assign, video, link } => {
                    put("type", Value::Str("vignette".into())); put("vignette_h", num(*thumb_h));
                    match video {
                        Some(Some(v)) => put("video", num(*v as i32 + 1)),
                        Some(None) => put("video", Value::Null),
                        None => if let Some(a) = assign { put("element", num(*a as i32 + 1)); },
                    }
                    if let Some(t) = link { put("cible", t.to_value()); }
                }
                Kind::Nav { action, text, target } => {
                    put("type", Value::Str("navigation".into())); put("action", Value::Str(action.name().into())); put("texte", Value::Str(text.clone()));
                    if let (NavAction::Goto, Some(t)) = (action, target) { put("cible", t.to_value()); }
                }
                Kind::Text { text, size, color, align, font, link } => {
                    if let Some(t) = link { put("cible", t.to_value()); }
                    if !font.is_empty() { put("police", Value::Str(font.clone())); }
                    put("type", Value::Str("texte".into())); put("texte", Value::Str(text.clone())); put("taille", num(*size));
                    put("couleur", Value::Str(format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]))); put("alignement", Value::Str(align.name().into()));
                }
                Kind::Image { path, link } => {
                    if let Some(t) = link { put("cible", t.to_value()); }
                    put("type", Value::Str("image".into())); put("fichier", Value::Str(path.clone()));
                }
            }
            put("x", num(it.x)); put("y", num(it.y)); put("w", num(it.w)); put("h", num(it.h));
            if let Some(st) = &it.look.style { put("style", st.to_value()); }
            if !it.look.skin.is_empty() { put("images_etats", it.look.skin.to_value()); }
            Value::Obj(o)
        }).collect();
        Value::Obj(vec![("elements".into(), Value::Arr(items))])
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let mut items = Vec::new();
        for (k, e) in v.get("elements").and_then(Value::as_array).ok_or("page sans « elements »")?.iter().enumerate() {
            let at = |key: &str| -> Result<i32, String> { e.get(key).and_then(Value::as_f64).map(|x| x.round() as i32).ok_or_else(|| format!("élément {} : « {key} » manquant", k + 1)) };
            let st = |key: &str| e.get(key).and_then(Value::as_str);
            let link = || -> Result<Option<Target>, String> {
                match e.get("cible") { None | Some(Value::Null) => Ok(None), Some(c) => Target::from_value(c).map(Some).ok_or_else(|| format!("élément {} : cible invalide", k + 1)) }
            };
            let kind = match st("type").ok_or_else(|| format!("élément {} sans type", k + 1))? {
                "vignette" => {
                    let one = |key: &str| e.get(key).and_then(Value::as_f64).map(|x| (x.round() as i64 - 1).max(0) as usize);
                    let video = match e.get("video") { None => None, Some(Value::Null) => Some(None), Some(_) => Some(one("video")) };
                    Kind::Slot { thumb_h: at("vignette_h")?, assign: one("element"), video, link: link()? }
                }
                "navigation" => {
                    let action = st("action").and_then(NavAction::from_name).ok_or_else(|| format!("élément {} : action inconnue", k + 1))?;
                    let target = match e.get("cible") {
                        Some(c) if action == NavAction::Goto => Some(Target::from_value(c).ok_or_else(|| format!("élément {} : cible invalide", k + 1))?),
                        _ => None,
                    };
                    Kind::Nav { action, text: st("texte").unwrap_or(action.default_text()).to_string(), target }
                }
                "texte" => {
                    let c = st("couleur").unwrap_or("#ffffff").trim_start_matches('#');
                    let rgb = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| format!("élément {} : couleur invalide", k + 1))?;
                    Kind::Text { text: st("texte").unwrap_or("").to_string(), size: at("taille")?, color: [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8],
                                 align: st("alignement").and_then(Align::from_name).unwrap_or(Align::Center), font: st("police").unwrap_or("").to_string(), link: link()? }
                }
                "image" => Kind::Image { path: st("fichier").ok_or_else(|| format!("élément {} : « fichier » manquant", k + 1))?.to_string(), link: link()? },
                t => return Err(format!("élément {} : type « {t} » inconnu", k + 1)),
            };
            let look = Look {
                style: match e.get("style") { Some(v @ Value::Obj(_)) => Some(ButtonStyle::from_value(v).map_err(|m| format!("élément {} : {m}", k + 1))?), _ => None },
                skin: e.get("images_etats").map(Skin::from_value).unwrap_or_default(),
            };
            items.push(Item { x: at("x")?, y: at("y")?, w: at("w")?, h: at("h")?, kind, look });
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
    /// Pages libres du menu principal, apres les pages automatiques : leurs vignettes montrent n'importe quelle video du projet
    /// (`assign` = indice de la video, obligatoire).
    pub free: Vec<PageDesign>,
    /// Apparence des boutons de tout le projet (chaque element peut avoir la sienne, voir `Item::look`).
    pub style: ButtonStyle,
}

impl Designs {
    pub fn is_empty(&self) -> bool { self.main.is_empty() && self.chapters.is_empty() && self.free.is_empty() && self.style == ButtonStyle::default() }

    /// Echange deux videos (les menus de chapitres suivent leur video).
    pub fn swap_videos(&mut self, a: usize, b: usize) {
        let old = std::mem::take(&mut self.chapters);
        self.chapters = old.into_iter().map(|((v, p), d)| (((if v == a { b } else if v == b { a } else { v }), p), d)).collect();
        for page in self.all_mut() {
            for it in &mut page.items {
                if let Kind::Slot { video: Some(Some(v)), .. } = &mut it.kind { *v = if *v == a { b } else if *v == b { a } else { *v }; }
            }
        }
        for d in self.all_mut() {
            for it in &mut d.items {
                if let Some(Some(t)) = it.targets_mut() { t.swap_videos(a, b); }
            }
        }
    }

    fn all_mut(&mut self) -> impl Iterator<Item = &mut PageDesign> {
        self.main.values_mut().chain(self.chapters.values_mut()).chain(self.free.iter_mut())
    }

    /// La page `page` (rang dans le menu principal, pages automatiques puis libres) vient d'etre supprimee : les boutons qui y
    /// menaient perdent leur cible, les suivants sont decales.
    pub fn page_removed(&mut self, page: usize) {
        for d in self.all_mut() {
            for it in &mut d.items {
                if let Some(target) = it.targets_mut() {
                    match *target {
                        Some(Target::Main(p)) if p == page => *target = None,
                        Some(Target::Main(p)) if p > page => *target = Some(Target::Main(p - 1)),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Retire une video : ses menus de chapitres disparaissent, les suivants sont renumerotes.
    pub fn remove_video(&mut self, i: usize) {
        let old = std::mem::take(&mut self.chapters);
        self.chapters = old.into_iter().filter(|((v, _), _)| *v != i).map(|((v, p), d)| ((if v > i { v - 1 } else { v }, p), d)).collect();
        // pages libres : les vignettes de cette video disparaissent, les suivantes sont renumerotees
        for d in self.all_mut() {
            for it in &mut d.items {
                if let Some(target) = it.targets_mut() {
                    match *target {
                        Some(Target::Chapters(v, _)) | Some(Target::Video(v)) if v == i => *target = None,
                        Some(Target::Chapters(v, q)) if v > i => *target = Some(Target::Chapters(v - 1, q)),
                        Some(Target::Video(v)) if v > i => *target = Some(Target::Video(v - 1)),
                        _ => {}
                    }
                }
            }
        }
        for page in self.all_mut() {
            // les vignettes de cette video restent en place, sans video ; les suivantes sont renumerotees
            for it in &mut page.items {
                if let Kind::Slot { video: Some(sv), .. } = &mut it.kind {
                    match *sv {
                        Some(v) if v == i => *sv = None,
                        Some(v) if v > i => *sv = Some(v - 1),
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn to_value(&self) -> Value {
        let mut main = Vec::new();
        for (p, d) in &self.main { main.push(((p + 1).to_string(), d.to_value())); }
        let mut by_video: BTreeMap<usize, Vec<(String, Value)>> = BTreeMap::new();
        for ((v, p), d) in &self.chapters { by_video.entry(*v).or_default().push(((p + 1).to_string(), d.to_value())); }
        let ch = by_video.into_iter().map(|(v, pages)| ((v + 1).to_string(), Value::Obj(pages))).collect();
        let mut o = vec![("principal".into(), Value::Obj(main)), ("chapitres".into(), Value::Obj(ch))];
        if !self.free.is_empty() { o.push(("libres".into(), Value::Arr(self.free.iter().map(PageDesign::to_value).collect()))); }
        if self.style != ButtonStyle::default() { o.push(("style_boutons".into(), self.style.to_value())); }
        Value::Obj(o)
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
        if let Some(libres) = v.get("libres").and_then(Value::as_array) {
            for (k, page) in libres.iter().enumerate() {
                let mut pd = PageDesign::from_value(page).map_err(|e| format!("page libre {} : {e}", k + 1))?;
                // anciens fichiers : sur une page libre, « element » designait deja la video
                for it in &mut pd.items {
                    if let Kind::Slot { assign, video, .. } = &mut it.kind {
                        if video.is_none() { *video = Some(assign.take()); }
                    }
                }
                d.free.push(pd);
            }
        }
        if let Some(st) = v.get("style_boutons") { d.style = ButtonStyle::from_value(st).map_err(|e| format!("style des boutons : {e}"))?; }
        Ok(d)
    }
}
