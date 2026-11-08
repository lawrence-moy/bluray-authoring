//! Mise en page personnalisee des pages de menu : vignettes, boutons de navigation, textes et images placees librement.
//!
//! Toutes les coordonnees sont exprimees sur un plan de reference 1920x1080 ; le constructeur les met a l'echelle de la taille du menu.
//! Sans mise en page personnalisee, une page garde la grille automatique (`default_main` / `default_chapter`), identique a celle de Python.
use crate::cover::{LineArrow, LineStyle, ShapeKind};
use crate::i18n::tr;
use crate::trf;
use crate::ig::py_round;
use crate::json::Value;
use crate::style::{ButtonStyle, Look, Skin, Weight};
use crate::project::MAX_PER_PAGE;

pub const PLANE: (i32, i32) = (1920, 1080);

/// Destination d'un bouton « Aller a », ou d'un texte / d'une image cliquable : une page du menu ou une video a lire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Page du menu (0-based).
    Main(usize),
    /// Lecture d'une video (0-based) depuis le debut.
    Video(usize),
    /// Regle les sous-titres (`Item::sub`) puis va a la page donnee (0-based) ; `None` = reste sur la page.
    Stay(Option<usize>),
    /// Regle la langue audio (`Language::id`) puis va a la page donnee ; `None` = reste sur la page. Jamais avec les sous-titres.
    Audio(u32, Option<usize>),
}

impl Target {
    /// Valeur JSON (champ « cible »).
    pub fn to_value(self) -> Value {
        let num = |v: usize| Value::Num((v + 1) as f64);
        let then = |e: Option<usize>| e.map(|p| ("ensuite".to_string(), num(p)));
        let o = match self {
            Target::Main(p) => vec![("page".to_string(), num(p))],
            Target::Video(v) => vec![("lire".to_string(), Value::Str("video".into())), ("video".to_string(), num(v))],
            Target::Stay(e) => std::iter::once(("rester".to_string(), Value::Bool(true))).chain(then(e)).collect(),
            Target::Audio(id, e) => std::iter::once(("audio_langue".to_string(), Value::Num(id as f64))).chain(then(e)).collect(),
        };
        Value::Obj(o)
    }
    pub fn from_value(c: &Value) -> Option<Target> {
        let n = |key: &str| c.get(key).and_then(Value::as_f64).map(|x| (x.round() as i64 - 1).max(0) as usize);
        if c.get("rester").and_then(Value::as_bool) == Some(true) {
            return Some(Target::Stay(n("ensuite")));
        }
        if let Some(id) = c.get("audio_langue").and_then(Value::as_i64) {
            return if id >= 1 { Some(Target::Audio(id as u32, n("ensuite"))) } else { None };
        }
        if c.get("lire").and_then(Value::as_str) == Some("video") {
            return n("video").map(Target::Video);
        }
        n("page").map(Target::Main)
    }
    /// Reglage sans navigation propre (sous-titres ou audio) ?
    pub fn is_setting(self) -> bool { matches!(self, Target::Stay(_) | Target::Audio(..)) }
    /// Page de sortie d'un reglage (`None` : reste sur la page, ou destination qui n'est pas un reglage).
    pub fn exit(self) -> Option<usize> { match self { Target::Stay(e) | Target::Audio(_, e) => e, _ => None } }
    /// Le meme reglage avec une autre page de sortie (sans effet sur les autres destinations).
    pub fn with_exit(self, e: Option<usize>) -> Target {
        match self { Target::Stay(_) => Target::Stay(e), Target::Audio(id, _) => Target::Audio(id, e), t => t }
    }
    /// La destination sans sa page de sortie : ce qui est choisi dans la liste « Destination ».
    pub fn without_exit(self) -> Target { self.with_exit(None) }
    /// Page visee (destination de page, ou page de sortie d'un reglage) : a renumeroter avec les pages.
    pub fn page_mut(&mut self) -> Option<&mut usize> {
        match self { Target::Main(p) | Target::Stay(Some(p)) | Target::Audio(_, Some(p)) => Some(p), _ => None }
    }
    /// La page `page` a ete supprimee : `false` si cette destination doit etre retiree (page visee),
    /// sinon renumerotation ; un reglage qui sortait vers cette page reste desormais sur place.
    pub fn page_removed(&mut self, page: usize) -> bool {
        match self {
            Target::Main(p) if *p == page => return false,
            Target::Stay(e) | Target::Audio(_, e) if *e == Some(page) => *e = None,
            _ => {}
        }
        if let Some(p) = self.page_mut() { if *p > page { *p -= 1; } }
        true
    }
    /// Renumerotation quand deux videos echangent leur place.
    fn swap_videos(&mut self, a: usize, b: usize) {
        let sw = |v: &mut usize| *v = if *v == a { b } else if *v == b { a } else { *v };
        match self { Target::Video(v) => sw(v), Target::Main(_) | Target::Stay(_) | Target::Audio(..) => {} }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavAction { Prev, Next, PlayAll, Goto }

impl NavAction {
    pub const ALL: [NavAction; 4] = [NavAction::Prev, NavAction::Next, NavAction::PlayAll, NavAction::Goto];
    pub fn name(self) -> &'static str {
        match self { NavAction::Prev => "precedent", NavAction::Next => "suivant", NavAction::PlayAll => "tout_lire", NavAction::Goto => "aller_a" }
    }
    pub fn from_name(s: &str) -> Option<Self> { Self::ALL.into_iter().find(|a| a.name() == s) }
    pub fn default_text(self) -> &'static str {
        match self { NavAction::Prev => tr("< Previous"), NavAction::Next => tr("Next >"), NavAction::PlayAll => tr("Play all"), NavAction::Goto => tr("Go to…") }
    }
    pub fn label(self) -> &'static str {
        match self { NavAction::Prev => tr("Previous page"), NavAction::Next => tr("Next page"), NavAction::PlayAll => tr("Play all"), NavAction::Goto => tr("Go to a page or play a video") }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align { Left, Center, Right }
impl Align {
    pub fn name(self) -> &'static str { match self { Align::Left => "gauche", Align::Center => "centre", Align::Right => "droite" } }
    pub fn from_name(s: &str) -> Option<Self> { [Align::Left, Align::Center, Align::Right].into_iter().find(|a| a.name() == s) }
}

/// Ou revenir a la fin de la video lancee par une vignette.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SlotReturn {
    /// la page ou se trouve la vignette
    #[default]
    Origin,
    /// la page d'accueil (touche TOP MENU)
    Home,
    /// une page donnee (indice dans la liste des pages)
    Page(usize),
}

impl SlotReturn {
    fn to_value(self) -> Option<Value> {
        match self {
            SlotReturn::Origin => None,
            SlotReturn::Home => Some(Value::Str("accueil".into())),
            SlotReturn::Page(p) => Some(Value::Obj(vec![("page".into(), Value::Num((p + 1) as f64))])),
        }
    }
    fn from_value(v: Option<&Value>) -> SlotReturn {
        match v {
            Some(Value::Str(s)) if s == "accueil" => SlotReturn::Home,
            Some(o @ Value::Obj(_)) => o.get("page").and_then(Value::as_f64).map_or(SlotReturn::Origin, |p| SlotReturn::Page((p.round() as i64 - 1).max(0) as usize)),
            _ => SlotReturn::Origin,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// Vignette cliquable + libelle ; `thumb_h` = hauteur de la vignette (le reste de `h` porte le libelle).
    /// `video` : video du projet montree (0-based) ; `None` = pas (ou plus) de video : la vignette n'apparait pas sur le disque.
    /// Une meme video peut etre sur plusieurs vignettes.
    /// `link` : destination au clic ; `None` = comportement habituel (video : son menu de chapitres s'il existe, sinon la lecture ;
    /// chapitre : lecture depuis ce chapitre).
    /// `start` : vignette de chapitre — la video demarre a cet instant (secondes) ; `None` = depuis le debut.
    /// `chapter` : identifiant du chapitre de la video auquel la vignette est liee (liste des chapitres de la video) ; `start`
    /// suit alors l'instant de ce chapitre (`Designs::sync_chapters`). `None` = instant libre.
    /// `label` : libelle (vide = titre de la video, ou « mm:ss » pour un chapitre) ; `thumb_at` : instant de l'image montree
    /// (`None` = depart du chapitre, ou image habituelle de la video) ; `ret` : ou revenir a la fin de la video.
    Slot { thumb_h: i32, video: Option<usize>, link: Option<Target>, start: Option<f64>, chapter: Option<u32>, label: String, thumb_at: Option<f64>, ret: SlotReturn },
    /// `target` : page visee par une action `Goto` (ignore pour les autres actions).
    Nav { action: NavAction, text: String, target: Option<Target> },
    /// `font` : vide = police par defaut ; sinon fichier .ttf/.otf (chemin absolu ou relatif au projet) ou nom d'un fichier de police installe.
    /// `link` : si present, le texte devient un bouton qui mene a cette destination.
    Text { text: String, size: i32, color: [u8; 3], align: Align, font: String, link: Option<Target>, deco: TextDeco },
    /// `link` : si present, l'image devient un bouton qui mene a cette destination.
    /// `frame` : image extraite d'une video du projet (« Image de la video ») au lieu du fichier `path` (vide).
    Image { path: String, link: Option<Target>, frame: Option<VideoFrame> },
    /// Rectangle (ou ellipse) et ligne dessines comme dans l'editeur de jaquette ; jamais cliquables.
    Shape { shape: ShapeKind, paint: Paint },
}

/// Trait et remplissage d'une forme (tailles en pixels d'un menu 1920 x 1080), comme `cover::CoverKind::Shape`.
/// `fill` / `stroke` : couleurs, `None` = absent ; la bordure est a l'interieur du cadre. Une ligne va d'un coin du cadre a
/// l'opposé (`flip` : du bas gauche au haut droit) ; `style` et `arrow` ne servent qu'aux lignes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    pub fill: Option<[u8; 3]>,
    pub stroke: Option<[u8; 3]>,
    pub stroke_px: i32,
    pub radius: i32,
    pub flip: bool,
    pub style: LineStyle,
    pub arrow: LineArrow,
    /// opacite en pourcentage (100 : opaque, 0 : invisible), pour le remplissage et le trait
    pub opacity: u8,
}

impl Paint {
    /// Rectangle plein blanc ; ligne blanche de 4 px.
    pub fn new(shape: ShapeKind) -> Paint {
        let white = Some([255, 255, 255]);
        let line = shape == ShapeKind::Line;
        Paint { fill: if line { None } else { white }, stroke: if line { white } else { None }, stroke_px: 4, radius: 0, flip: false, style: LineStyle::Solid, arrow: LineArrow::None, opacity: 100 }
    }
    /// Opacite entre 0 et 1.
    pub fn alpha(&self) -> f32 { self.opacity.min(100) as f32 / 100.0 }
    /// Meme dessin a l'echelle `k` (epaisseur de trait d'au moins 1 px).
    pub fn scaled(&self, k: f64) -> Paint {
        let r = |v: i32| if v <= 0 { 0 } else { ((v as f64 * k).round() as i32).max(1) };
        Paint { stroke_px: r(self.stroke_px).max(1), radius: r(self.radius), ..*self }
    }
    /// Couleur d'une ligne : celle du trait, a defaut celle du remplissage.
    pub fn line_color(&self) -> Option<[u8; 3]> { self.stroke.or(self.fill) }
    fn put(&self, shape: ShapeKind, put: &mut dyn FnMut(&str, Value)) {
        if let Some(c) = self.fill { put("remplissage", hex(c)); }
        if let Some(c) = self.stroke { put("bordure", hex(c)); put("epaisseur", Value::Num(self.stroke_px as f64)); }
        if self.radius > 0 && shape == ShapeKind::Rect { put("arrondi", Value::Num(self.radius as f64)); }
        if self.opacity < 100 { put("opacite", Value::Num(self.opacity as f64)); }
        if shape == ShapeKind::Line {
            if self.flip { put("retourne", Value::Bool(true)); }
            if self.style != LineStyle::Solid { put("trait", Value::Str(self.style.name().into())); }
            if self.arrow != LineArrow::None { put("fleche", Value::Str(self.arrow.name().into())); }
        }
    }
    fn from_item(e: &Value) -> Result<Paint, String> {
        let col = |k: &str| -> Result<Option<[u8; 3]>, String> {
            match e.get(k).and_then(Value::as_str) { None => Ok(None), Some(s) => parse_hex(s).map(Some).ok_or_else(|| trf!("invalid color: {}", s)) }
        };
        let int = |k: &str, d: i32| e.get(k).and_then(Value::as_f64).map_or(d, |x| x.round().max(0.0) as i32);
        Ok(Paint {
            fill: col("remplissage")?,
            stroke: col("bordure")?,
            stroke_px: int("epaisseur", 4).clamp(1, 200),
            radius: int("arrondi", 0).min(540),
            flip: e.get("retourne").and_then(Value::as_bool).unwrap_or(false),
            style: e.get("trait").and_then(Value::as_str).and_then(LineStyle::from_name).unwrap_or(LineStyle::Solid),
            arrow: e.get("fleche").and_then(Value::as_str).and_then(LineArrow::from_name).unwrap_or(LineArrow::None),
            opacity: e.get("opacite").and_then(Value::as_f64).map_or(100, |x| x.round().clamp(0.0, 100.0) as u8),
        })
    }
}

/// Image d'une video du projet : `video` (rang, `None` : a choisir), instant `at` en secondes, recadrage `crop` (x, y, largeur,
/// hauteur en fractions de l'image entiere ; `None` : centre de l'image en 16/9).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoFrame {
    pub video: Option<usize>,
    pub at: f64,
    pub crop: Option<[f64; 4]>,
}

impl VideoFrame {
    /// Zone gardee (x, y, largeur, hauteur en pixels) d'une image de `w` x `h` pixels.
    pub fn crop_px(&self, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let (wf, hf) = (w.max(1) as f64, h.max(1) as f64);
        match self.crop {
            Some([x, y, cw, ch]) => {
                let x0 = (x.clamp(0.0, 1.0) * wf).round();
                let y0 = (y.clamp(0.0, 1.0) * hf).round();
                let x1 = ((x + cw).clamp(0.0, 1.0) * wf).round().max(x0 + 1.0).min(wf);
                let y1 = ((y + ch).clamp(0.0, 1.0) * hf).round().max(y0 + 1.0).min(hf);
                (x0.min(wf - 1.0) as u32, y0.min(hf - 1.0) as u32, (x1 - x0.min(wf - 1.0)).max(1.0) as u32, (y1 - y0.min(hf - 1.0)).max(1.0) as u32)
            }
            None => {
                // comme ffmpeg (force_original_aspect_ratio=increase puis crop) : la plus grande zone 16/9 centree
                let r = 16.0 / 9.0;
                if wf / hf > r { let cw = (hf * r).round(); (((wf - cw) / 2.0).round() as u32, 0, cw as u32, h) } else { let ch = (wf / r).round(); (0, ((hf - ch) / 2.0).round() as u32, w, ch as u32) }
            }
        }
    }
}

/// Habillage d'un texte : fond derriere le cadre et contour des lettres (tailles en pixels d'un menu 1920 x 1080).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextDeco {
    pub bg_color: [u8; 3],
    /// opacite du fond en % (0 : pas de fond)
    pub bg_opacity: u8,
    /// rayon des coins du fond
    pub bg_radius: i32,
    /// epaisseur du contour des lettres (0 : aucun)
    pub outline: i32,
    pub outline_color: [u8; 3],
}

fn hex(c: [u8; 3]) -> Value { Value::Str(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])) }

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let c = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6)?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

impl TextDeco {
    pub fn is_plain(&self) -> bool { self.bg_opacity == 0 && self.outline <= 0 }
    /// Meme habillage a l'echelle `k` (le facteur applique aux positions et a la taille du texte).
    pub fn scaled(&self, k: f64) -> TextDeco {
        let r = |v: i32| if v <= 0 { 0 } else { ((v as f64 * k).round() as i32).max(1) };
        TextDeco { bg_radius: r(self.bg_radius), outline: r(self.outline), ..*self }
    }
    /// Champs JSON de l'element (seulement ceux qui servent).
    fn put(&self, put: &mut dyn FnMut(&str, Value)) {
        if self.bg_opacity > 0 {
            put("fond_couleur", hex(self.bg_color));
            put("fond_opacite", Value::Num(self.bg_opacity.min(100) as f64));
            if self.bg_radius > 0 { put("fond_arrondi", Value::Num(self.bg_radius as f64)); }
        }
        if self.outline > 0 {
            put("contour", Value::Num(self.outline as f64));
            put("contour_couleur", hex(self.outline_color));
        }
    }
    fn from_item(e: &Value) -> Result<TextDeco, String> {
        let col = |k: &str, d: [u8; 3]| -> Result<[u8; 3], String> {
            match e.get(k).and_then(Value::as_str) { None => Ok(d), Some(s) => parse_hex(s).ok_or_else(|| trf!("invalid color: {}", s)) }
        };
        let int = |k: &str| e.get(k).and_then(Value::as_f64).map_or(0, |x| x.round().max(0.0) as i32);
        Ok(TextDeco {
            bg_color: col("fond_couleur", [0, 0, 0])?,
            bg_opacity: int("fond_opacite").min(100) as u8,
            bg_radius: int("fond_arrondi").min(540),
            outline: int("contour").min(60),
            outline_color: col("contour_couleur", [0, 0, 0])?,
        })
    }
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
    /// Calque de l'editeur (identifiant dans `PageDesign::layers`) ; 0 ou inconnu : premier calque. Sans effet sur le disque.
    pub layer: u32,
    /// Sous-titres regles par le clic sur cet element (avant la lecture qu'il lance).
    pub sub: SubChoice,
    /// Identifiant stable dans la page (0 = pas encore attribue) : les liens de navigation designent un element par cet identifiant.
    pub id: u32,
    /// Voisins choisis a la main pour les fleches de la telecommande (sinon calcules automatiquement).
    pub nav: NavLinks,
    /// Nom donne par l'auteur (arbre des elements, choix de navigation) ; vide = nom par defaut.
    pub name: String,
}

/// Destination d'une fleche de la telecommande depuis un bouton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavLink {
    /// calculee automatiquement (le plus proche dans cette direction)
    #[default]
    Auto,
    /// la fleche ne deplace pas la selection
    Stay,
    /// bouton de la page, par identifiant d'element
    Item(u32),
}

/// Les quatre fleches (haut, bas, gauche, droite) d'un bouton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NavLinks {
    pub up: NavLink,
    pub down: NavLink,
    pub left: NavLink,
    pub right: NavLink,
}

impl NavLinks {
    pub const KEYS: [&'static str; 4] = ["haut", "bas", "gauche", "droite"];
    pub fn is_auto(&self) -> bool { *self == NavLinks::default() }
    pub fn get(&self, d: usize) -> NavLink { [self.up, self.down, self.left, self.right][d] }
    pub fn set(&mut self, d: usize, l: NavLink) {
        match d { 0 => self.up = l, 1 => self.down = l, 2 => self.left = l, _ => self.right = l }
    }
    pub fn to_value(&self) -> Option<Value> {
        let mut o = Vec::new();
        for (d, k) in Self::KEYS.iter().enumerate() {
            match self.get(d) {
                NavLink::Auto => {}
                NavLink::Stay => o.push((k.to_string(), Value::Str("aucun".into()))),
                NavLink::Item(id) => o.push((k.to_string(), Value::Num(id as f64))),
            }
        }
        if o.is_empty() { None } else { Some(Value::Obj(o)) }
    }
    pub fn from_value(v: Option<&Value>) -> NavLinks {
        let mut n = NavLinks::default();
        if let Some(v) = v {
            for (d, k) in Self::KEYS.iter().enumerate() {
                match v.get(k) {
                    Some(Value::Str(s)) if s == "aucun" => n.set(d, NavLink::Stay),
                    Some(x) => if let Some(id) = x.as_f64().filter(|i| *i >= 1.0) { n.set(d, NavLink::Item(id as u32)); },
                    None => {}
                }
            }
        }
        n
    }
}

/// Sous-titres choisis par un bouton.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SubChoice {
    /// rien ne change : choix du spectateur (touche SUBTITLE ou autre bouton)
    #[default]
    Keep,
    /// sous-titres masques
    Off,
    /// langue de la liste du projet (`Language::id`) : meme numero de piste dans toutes les videos
    Lang(u32),
}

impl SubChoice {
    fn to_value(self) -> Option<Value> {
        match self {
            SubChoice::Keep => None,
            SubChoice::Off => Some(Value::Str("aucun".into())),
            SubChoice::Lang(id) => Some(Value::Obj(vec![("langue".into(), Value::Num(id as f64))])),
        }
    }
    fn from_value(v: Option<&Value>) -> SubChoice {
        match v {
            Some(Value::Str(s)) if s == "aucun" => SubChoice::Off,
            Some(o @ Value::Obj(_)) => match o.get("langue").and_then(Value::as_i64) { Some(id) if id >= 1 => SubChoice::Lang(id as u32), _ => SubChoice::Keep },
            _ => SubChoice::Keep,
        }
    }
}

impl Item {
    pub fn is_button(&self) -> bool { matches!(self.kind, Kind::Slot { .. } | Kind::Nav { .. }) || self.link().is_some() }
    /// Destination d'un texte ou d'une image cliquable.
    pub fn link(&self) -> Option<Target> {
        match self.kind { Kind::Text { link, .. } | Kind::Image { link, .. } | Kind::Slot { link, .. } => link, _ => None }
    }
    /// Vignette : indice de la video montree.
    pub fn fixed_video(&self) -> Option<usize> { if let Kind::Slot { video, .. } = self.kind { video } else { None } }
    /// Vignette d'une video du projet (`start` : instant de depart pour une vignette de chapitre).
    pub fn video_slot(x: i32, y: i32, w: i32, h: i32, thumb_h: i32, video: usize, start: Option<f64>, label: &str) -> Item {
        Item { x, y, w, h, kind: Kind::Slot { thumb_h, video: Some(video), link: None, start, chapter: None, label: label.into(), thumb_at: None, ret: SlotReturn::Origin }, look: Look::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() }
    }
    /// « Image de la video » : image de la video `video` a l'instant `at` (secondes).
    pub fn video_frame(x: i32, y: i32, w: i32, h: i32, video: Option<usize>, at: f64) -> Item {
        Item { x, y, w, h, kind: Kind::Image { path: String::new(), link: None, frame: Some(VideoFrame { video, at, crop: None }) }, look: Look::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() }
    }
    /// Instant de depart d'une vignette de chapitre.
    pub fn chapter_start(&self) -> Option<f64> {
        if let Kind::Slot { start, .. } = self.kind { start } else { None }
    }
    /// Video que lance un clic sur l'element (bouton « Aller a » ou lien vers une video, vignette sans autre destination).
    pub fn played_video(&self) -> Option<usize> {
        match &self.kind {
            Kind::Nav { action: NavAction::Goto, target: Some(Target::Video(v)), .. } => Some(*v),
            Kind::Text { link: Some(Target::Video(v)), .. } | Kind::Image { link: Some(Target::Video(v)), .. } => Some(*v),
            Kind::Slot { link: Some(Target::Video(v)), .. } => Some(*v),
            Kind::Slot { link: None, .. } => self.fixed_video(),
            _ => None,
        }
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
    /// Rectangle ou ligne (voir `Paint::new`) ; une ligne est tracee de son coin haut gauche au coin bas droit du cadre.
    pub fn shape(shape: ShapeKind, x: i32, y: i32, w: i32, h: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Shape { shape, paint: Paint::new(shape) }, look: Look::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() }
    }
    pub fn text(text: &str, x: i32, y: i32, w: i32, h: i32, size: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Text { text: text.into(), size, color: [255, 255, 255], align: Align::Center, font: String::new(), link: None, deco: TextDeco::default() }, look: Look::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() }
    }
    pub fn nav(action: NavAction, x: i32, y: i32, w: i32, h: i32) -> Item {
        Item { x, y, w, h, kind: Kind::Nav { action, text: action.default_text().into(), target: if action == NavAction::Goto { Some(Target::Main(0)) } else { None } }, look: Look::default(), layer: 0, sub: Default::default(), id: 0, nav: Default::default(), name: String::new() }
    }
}

/// Visuel du fond d'une page.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PageVisual {
    /// celui du projet (image, video ou fond genere)
    #[default]
    Project,
    Image(String),
    /// video jouee en boucle ; `bool` : garder son son (a defaut de musique)
    Video(String, bool),
}

/// Musique d'une page.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PageMusic {
    /// celle du projet
    #[default]
    Project,
    Silent,
    File(String),
}

/// Fond propre a une page (sinon : celui du projet).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageBg {
    pub visual: PageVisual,
    pub music: PageMusic,
}

impl PageBg {
    pub fn is_default(&self) -> bool { *self == PageBg::default() }
    pub fn to_value(&self) -> Value {
        let mut o: Vec<(String, Value)> = Vec::new();
        match &self.visual {
            PageVisual::Project => {}
            PageVisual::Image(p) => o.push(("image".into(), Value::Str(p.clone()))),
            PageVisual::Video(p, a) => {
                o.push(("video".into(), Value::Str(p.clone())));
                if *a { o.push(("son_video".into(), Value::Bool(true))); }
            }
        }
        match &self.music {
            PageMusic::Project => {}
            PageMusic::Silent => o.push(("musique".into(), Value::Null)),
            PageMusic::File(p) => o.push(("musique".into(), Value::Str(p.clone()))),
        }
        Value::Obj(o)
    }
    pub fn from_value(v: &Value) -> PageBg {
        let st = |k: &str| v.get(k).and_then(Value::as_str).filter(|x| !x.is_empty()).map(String::from);
        let visual = match (st("video"), st("image")) {
            (Some(p), _) => PageVisual::Video(p, v.get("son_video").and_then(Value::as_bool).unwrap_or(false)),
            (None, Some(p)) => PageVisual::Image(p),
            _ => PageVisual::Project,
        };
        let music = match v.get("musique") {
            None => PageMusic::Project,
            Some(Value::Str(p)) if !p.is_empty() => PageMusic::File(p.clone()),
            Some(_) => PageMusic::Silent,
        };
        PageBg { visual, music }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageDesign {
    /// Elements, de l'arriere-plan au premier plan. Avec des calques, les elements d'un meme calque se suivent (voir
    /// `normalize_layers`).
    pub items: Vec<Item>,
    /// Calques de l'editeur, de l'arriere-plan au premier plan (vide : pas de calques). Ils regroupent les elements pour les
    /// ranger, les masquer dans l'apercu ou les verrouiller ; le disque ne depend que de l'ordre des elements.
    pub layers: Vec<Layer>,
    /// Nom de la page dans l'editeur (facultatif).
    pub name: String,
    /// Fond propre a cette page ; par defaut celui du projet.
    pub bg: PageBg,
    /// Cadence des animations de boutons de la page (images par seconde, une seule par page selon la norme) ; 0 = 8 par defaut.
    pub anim_fps: u32,
    /// Lancement automatique : au bout de `seconds` secondes passees sur la page sans clic, l'element `item` est active.
    pub idle: Option<Idle>,
}

/// Lancement automatique d'un element de la page apres un delai (voir `PageDesign::idle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Idle {
    /// delai en secondes (1 a `MAX_IDLE_SECONDS`)
    pub seconds: u32,
    /// identifiant (`Item::id`) de l'element active
    pub item: u32,
}

/// Plus long delai de lancement automatique.
pub const MAX_IDLE_SECONDS: u32 = 3600;

/// Cadence des animations de boutons quand la page n'en precise pas.
pub const DEFAULT_ANIM_FPS: u32 = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct Problem { pub error: bool, pub msg: String }

/// Ce bouton de navigation existe-t-il sur cette page ?
pub fn nav_applicable(action: NavAction, page: usize, pages: usize) -> bool {
    match action {
        NavAction::Prev => page > 0,
        NavAction::Next => page + 1 < pages,
        NavAction::PlayAll | NavAction::Goto => true,
    }
}

pub(crate) const NAV_Y: i32 = 1080 - 60 - 80;

impl PageDesign {
    /// Nombre de vignettes de la page.
    pub fn slot_count(&self) -> usize { self.items.iter().filter(|i| matches!(i.kind, Kind::Slot { .. })).count() }

    /// Verifie la page `page` (0-based) sur `pages` ; `n_videos` videos dans le projet.
    pub fn validate(&self, page: usize, pages: usize, n_videos: usize) -> Vec<Problem> {
        let mut p = self.validate_items(page, pages, n_videos);
        for (a, b) in self.overlapping(page, pages) { p.push(Problem { error: false, msg: trf!("Items {} and {} overlap.", a + 1, b + 1) }); }
        p
    }

    /// Paires d'elements (indices) qui se chevauchent parmi ceux qu'on peut activer a la telecommande : boutons, vignettes, liens.
    pub fn overlapping(&self, page: usize, pages: usize) -> Vec<(usize, usize)> {
        let active: Vec<(usize, &Item)> = self.items.iter().enumerate().filter(|(_, i)| match &i.kind {
            Kind::Slot { .. } => true,
            Kind::Nav { action, .. } => nav_applicable(*action, page, pages),
            _ => i.link().is_some(),
        }).collect();
        let mut out = Vec::new();
        for (ai, (a, ia)) in active.iter().enumerate() {
            for (b, ib) in &active[ai + 1..] {
                if ia.x < ib.x + ib.w && ib.x < ia.x + ia.w && ia.y < ib.y + ib.h && ib.y < ia.y + ia.h { out.push((*a, *b)); }
            }
        }
        out
    }

    /// Textes non cliquables poses sur un bouton : (texte, bouton). Pas une erreur (le texte fait partie du fond), mais l'image du
    /// bouton est dessinee par-dessus et peut le cacher ; l'editeur les signale en orange.
    pub fn covered_texts(&self, page: usize, pages: usize) -> Vec<(usize, usize)> {
        let button = |i: &Item| match &i.kind {
            Kind::Slot { .. } => true,
            Kind::Nav { action, .. } => nav_applicable(*action, page, pages),
            _ => i.link().is_some(),
        };
        let mut out = Vec::new();
        for (a, ia) in self.items.iter().enumerate() {
            if !matches!(ia.kind, Kind::Text { .. }) || button(ia) { continue; }
            for (b, ib) in self.items.iter().enumerate() {
                if a != b && button(ib) && ia.x < ib.x + ib.w && ib.x < ia.x + ia.w && ia.y < ib.y + ib.h && ib.y < ia.y + ia.h { out.push((a, b)); }
            }
        }
        out
    }

    /// Comme `validate`, sans les chevauchements (l'editeur les montre par des contours sur l'apercu : voir `overlapping`).
    pub fn validate_items(&self, page: usize, pages: usize, n_videos: usize) -> Vec<Problem> {
        let mut p = Vec::new();
        let (err, warn) = (|m: String| Problem { error: true, msg: m }, |m: String| Problem { error: false, msg: m });
        if let Some(i) = self.idle {
            // l'element doit etre un bouton de la page
            if !self.items.iter().any(|it| it.id == i.item && it.id != 0 && it.is_button()) {
                p.push(err(tr("Automatic launch: choose an item of the page that is a button.").to_string()));
            }
        }
        let all = self.slot_count();
        if all > MAX_PER_PAGE { p.push(err(trf!("{} thumbnails: {} at most per page.", all, MAX_PER_PAGE))); }
        let mut slot_no = 0;
        for (k, it) in self.items.iter().enumerate() {
            // une ligne peut etre parfaitement horizontale ou verticale (un cote nul)
            let line = matches!(it.kind, Kind::Shape { shape: ShapeKind::Line, .. });
            if if line { it.w.max(it.h) < 8 || it.w < 0 || it.h < 0 } else { it.w < 8 || it.h < 8 } { p.push(err(trf!("Item {} too small.", k + 1))); }
            if it.x < 0 || it.y < 0 || it.x + it.w > PLANE.0 || it.y + it.h > PLANE.1 {
                let m = trf!("Item {} extends beyond the screen.", k + 1);
                p.push(if it.is_button() { err(m) } else { warn(m) });
            }
            if let Kind::Slot { thumb_h, video, .. } = it.kind {
                slot_no += 1;
                if thumb_h < 8 || thumb_h > it.h { p.push(err(trf!("Item {}: inconsistent thumbnail height.", k + 1))); }
                match video {
                    None => p.push(warn(trf!("Thumbnail {}: without video, it will not appear on the disc.", slot_no))),
                    Some(v) if v >= n_videos => p.push(err(trf!("Thumbnail {}: video {} does not exist.", slot_no, v + 1))),
                    _ => {}
                }
            }
            if let Kind::Image { frame: Some(f), .. } = it.kind {
                match f.video {
                    None => p.push(warn(trf!("Item {}: choose the video of the image.", k + 1))),
                    Some(v) if v >= n_videos => p.push(err(trf!("Item {}: video {} does not exist.", k + 1, v + 1))),
                    _ => {}
                }
            }
            for d in 0..4 {
                if let NavLink::Item(id) = it.nav.get(d) {
                    let name = ["haut", "bas", "gauche", "droite"][d];
                    match self.item_by_id(id) {
                        None => p.push(warn(trf!("Item {}: arrow {} leads to a deleted item (automatic navigation instead).", k + 1, name))),
                        Some(t) if t == k => p.push(warn(trf!("Item {}: arrow {} leads to itself.", k + 1, name))),
                        _ => {}
                    }
                }
            }
            if let Kind::Nav { action: NavAction::Goto, target: None, .. } = it.kind {
                p.push(err(trf!("Item {}: choose the button's target page.", k + 1)));
            }
            if let Kind::Nav { action, .. } = it.kind {
                if !nav_applicable(action, page, pages) { p.push(warn(trf!("“{}” will not appear on this page.", action.label()))); }
            }
        }
        // impasse : aucun bouton ni lien ne mene a une autre page (la touche Menu ramene toujours a l'accueil)
        let exits = self.items.iter().any(|i| match &i.kind {
            Kind::Nav { action: NavAction::Prev | NavAction::Next | NavAction::Goto, .. } => true,
            _ => matches!(i.link(), Some(Target::Main(_) | Target::Stay(Some(_)) | Target::Audio(_, Some(_)))),
        });
        if !exits && page > 0 {
            p.push(warn(tr("No button to another page: the only way out is the remote control's Menu key.").into()));
        }
        p
    }

    /// Verifie les pages visees par les boutons « Aller a » ; `exists` dit si une page existe dans le projet.
    pub fn check_targets(&self, exists: &dyn Fn(Target) -> bool) -> Vec<Problem> {
        let mut p = Vec::new();
        for (k, it) in self.items.iter().enumerate() {
            let t = match it.kind { Kind::Nav { action: NavAction::Goto, target: Some(t), .. } => Some(t), _ => it.link() };
            if let Some(t) = t {
                if !exists(t) { p.push(Problem { error: true, msg: trf!("Item {}: the destination does not exist.", k + 1) }); }
            }
            if let Some(v) = it.fixed_video() {
                if !exists(Target::Video(v)) { p.push(Problem { error: true, msg: trf!("Item {}: video {} does not exist.", k + 1, v + 1) }); }
            }
        }
        p
    }

    /// Poids estime de la page `page` sur `pages` : images de boutons a decoder et delai d'affichage. `base` = style du projet,
    /// `plane` = taille du menu en pixels.
    pub fn weight(&self, base: &ButtonStyle, page: usize, pages: usize, plane: (u32, u32)) -> Weight {
        let k = plane.0 as f64 / PLANE.0 as f64;
        let (mut bytes, mut buttons) = (0u64, 0usize);
        for it in &self.items {
            let shown = match &it.kind {
                Kind::Slot { video, .. } => video.is_some(),
                Kind::Nav { action, .. } => nav_applicable(*action, page, pages),
                _ => it.link().is_some(),
            };
            if !shown { continue; }
            let st = it.look.style.as_ref().unwrap_or(base).scaled(k);
            let (_, _, w, h) = it.scaled(k);
            let (mx, my) = st.margins(w, h);
            let slot = matches!(it.kind, Kind::Slot { .. });
            let anim = if slot { 0 } else { it.look.skin.anim_frames() as u64 };
            // animation de l'etat normal : ses images (exemplaires propres) s'ajoutent a l'image normale de reference
            let nanim = if slot || !it.look.skin.has_normal() { 0 } else { it.look.skin.normal_anim_frames() as u64 };
            let states = nanim + if anim > 0 {
                // image normale + animation (exemplaires propres) + etat active s'il est distinct
                1 + anim + u64::from(!it.look.skin.activated.is_empty() || st.activated)
            } else if !it.look.skin.activated.is_empty() { 3 + st.pulse_frames() as u64 - 1 } else { st.state_count() };
            bytes += ((w + 2 * mx).max(1) as u64) * ((h + 2 * my).max(1) as u64) * states;
            buttons += 1;
        }
        let plane_px = plane.0 as u64 * plane.1 as u64;
        Weight { bytes, seconds: (plane_px + bytes) as f64 / crate::style::DECODE_RATE, buttons }
    }

    // ---------------------------------------------------------------- calques
    /// Position (dans `layers`) du calque de l'element `i` ; 0 sans calques ou pour un calque inconnu.
    pub fn layer_pos(&self, i: usize) -> usize {
        let id = self.items.get(i).map_or(0, |it| it.layer);
        self.layers.iter().position(|l| l.id == id).unwrap_or(0)
    }
    /// Indices (dans `items`) des elements du calque en position `pos` (consecutifs apres `normalize_layers`).
    pub fn layer_range(&self, pos: usize) -> std::ops::Range<usize> {
        let a = (0..self.items.len()).find(|&i| self.layer_pos(i) >= pos).unwrap_or(self.items.len());
        let b = (a..self.items.len()).find(|&i| self.layer_pos(i) > pos).unwrap_or(self.items.len());
        a..b
    }
    /// Element masque ou verrouille par son calque ?
    pub fn item_hidden(&self, i: usize) -> bool { !self.layers.is_empty() && self.layers[self.layer_pos(i)].hidden }
    pub fn item_locked(&self, i: usize) -> bool { !self.layers.is_empty() && self.layers[self.layer_pos(i)].locked }
    /// Rattache chaque element a un calque existant (le premier par defaut) et regroupe les elements de chaque calque, dans
    /// l'ordre des calques, sans changer leur ordre a l'interieur d'un calque. Retourne la nouvelle place de chaque element.
    pub fn normalize_layers(&mut self) -> Vec<usize> {
        if self.layers.is_empty() {
            for it in &mut self.items { it.layer = 0; }
            return (0..self.items.len()).collect();
        }
        let first = self.layers[0].id;
        for it in &mut self.items {
            if !self.layers.iter().any(|l| l.id == it.layer) { it.layer = first; }
        }
        let mut order: Vec<usize> = (0..self.items.len()).collect();
        order.sort_by_key(|&i| self.layer_pos(i));
        let mut place = vec![0; order.len()];
        for (new, &old) in order.iter().enumerate() { place[old] = new; }
        let items = std::mem::take(&mut self.items);
        let mut slots: Vec<Option<Item>> = items.into_iter().map(Some).collect();
        self.items = order.iter().map(|&i| slots[i].take().unwrap()).collect();
        place
    }

    /// Page libre vierge : seulement le bouton « Précédent ».
    pub fn blank_free(page: usize) -> Self {
        let mut items = Vec::new();
        if page > 0 { items.push(Item::nav(NavAction::Prev, 560, NAV_Y, 360, 80)); }
        PageDesign { items, ..Default::default() }
    }

    // ---------------------------------------------------------------- JSON
    pub fn to_value(&self) -> Value {
        let num = |v: i32| Value::Num(v as f64);
        let items = self.items.iter().map(|it| {
            let mut o: Vec<(String, Value)> = Vec::new();
            let mut put = |k: &str, v: Value| o.push((k.to_string(), v));
            match &it.kind {
                Kind::Slot { thumb_h, video, link, start, chapter, label, thumb_at, ret } => {
                    put("type", Value::Str("vignette".into())); put("vignette_h", num(*thumb_h));
                    put("video", video.map_or(Value::Null, |v| num(v as i32 + 1)));
                    if let Some(t) = link { put("cible", t.to_value()); }
                    if let Some(t) = start { put("depart_s", Value::Num(*t)); }
                    if let (Some(c), Some(_)) = (chapter, start) { put("chapitre", num(*c as i32)); }
                    if !label.is_empty() { put("libelle", Value::Str(label.clone())); }
                    if let Some(t) = thumb_at { put("image_s", Value::Num(*t)); }
                    if let Some(r) = ret.to_value() { put("retour", r); }
                }
                Kind::Nav { action, text, target } => {
                    put("type", Value::Str("navigation".into())); put("action", Value::Str(action.name().into())); put("texte", Value::Str(text.clone()));
                    if let (NavAction::Goto, Some(t)) = (action, target) { put("cible", t.to_value()); }
                }
                Kind::Text { text, size, color, align, font, link, deco } => {
                    if let Some(t) = link { put("cible", t.to_value()); }
                    deco.put(&mut |k, v| put(k, v));
                    if !font.is_empty() { put("police", Value::Str(font.clone())); }
                    put("type", Value::Str("texte".into())); put("texte", Value::Str(text.clone())); put("taille", num(*size));
                    put("couleur", Value::Str(format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]))); put("alignement", Value::Str(align.name().into()));
                }
                Kind::Shape { shape, paint } => {
                    put("type", Value::Str("forme".into())); put("forme", Value::Str(shape.name().into()));
                    paint.put(*shape, &mut |k, v| put(k, v));
                }
                Kind::Image { path, link, frame } => {
                    if let Some(t) = link { put("cible", t.to_value()); }
                    put("type", Value::Str("image".into()));
                    match frame {
                        Some(f) => {
                            if let Some(v) = f.video { put("video", num(v as i32 + 1)); }
                            put("instant", Value::Num(f.at));
                            if let Some(c) = f.crop { put("recadrage", Value::Arr(c.iter().map(|v| Value::Num((v * 1e4).round() / 1e4)).collect())); }
                        }
                        None => put("fichier", Value::Str(path.clone())),
                    }
                }
            }
            put("x", num(it.x)); put("y", num(it.y)); put("w", num(it.w)); put("h", num(it.h));
            if let Some(st) = &it.look.style { put("style", st.to_value()); }
            if !it.look.skin.is_empty() { put("images_etats", it.look.skin.to_value()); }
            if let Some(s) = &it.look.sounds { put("sons", s.to_value()); }
            if !self.layers.is_empty() { put("calque", num(it.layer as i32)); }
            if let Some(v) = it.sub.to_value() { put("soustitres", v); }
            if !it.name.is_empty() { put("nom", Value::Str(it.name.clone())); }
            if it.id != 0 { put("id", num(it.id as i32)); }
            if let Some(v) = it.nav.to_value() { put("navigation", v); }
            Value::Obj(o)
        }).collect();
        let mut o = vec![("elements".into(), Value::Arr(items))];
        if !self.layers.is_empty() {
            let ls = self.layers.iter().map(|l| {
                let mut o = vec![("id".to_string(), num(l.id as i32)), ("nom".to_string(), Value::Str(l.name.clone()))];
                if l.hidden { o.push(("masque".into(), Value::Bool(true))); }
                if l.locked { o.push(("verrouille".into(), Value::Bool(true))); }
                Value::Obj(o)
            }).collect();
            o.push(("calques".into(), Value::Arr(ls)));
        }
        if !self.name.is_empty() { o.push(("nom".into(), Value::Str(self.name.clone()))); }
        if self.anim_fps != 0 { o.push(("animation_ips".into(), Value::Num(self.anim_fps as f64))); }
        if let Some(i) = self.idle { o.push(("lancement_auto".into(), Value::Obj(vec![("secondes".into(), Value::Num(i.seconds as f64)), ("element".into(), Value::Num(i.item as f64))]))); }
        if !self.bg.is_default() { o.push(("fond_page".into(), self.bg.to_value())); }
        Value::Obj(o)
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let mut items = Vec::new();
        for (k, e) in v.get("elements").and_then(Value::as_array).ok_or(tr("page without “elements”"))?.iter().enumerate() {
            let at = |key: &str| -> Result<i32, String> { e.get(key).and_then(Value::as_f64).map(|x| x.round() as i32).ok_or_else(|| trf!("item {}: “{}” missing", k + 1, key)) };
            let st = |key: &str| e.get(key).and_then(Value::as_str);
            let link = || -> Result<Option<Target>, String> {
                match e.get("cible") { None | Some(Value::Null) => Ok(None), Some(c) => Target::from_value(c).map(Some).ok_or_else(|| trf!("item {}: invalid target", k + 1)) }
            };
            let kind = match st("type").ok_or_else(|| trf!("item {} without type", k + 1))? {
                "vignette" => {
                    let one = |key: &str| e.get(key).and_then(Value::as_f64).map(|x| (x.round() as i64 - 1).max(0) as usize);
                    let video = one("video");
                    let num = |key: &str| e.get(key).and_then(Value::as_f64).map(|x| x.max(0.0));
                    Kind::Slot { thumb_h: at("vignette_h")?, video, link: link()?, start: num("depart_s"),
                                 chapter: num("depart_s").and_then(|_| e.get("chapitre").and_then(Value::as_f64)).map(|c| c.max(0.0) as u32), label: st("libelle").unwrap_or("").to_string(),
                                 thumb_at: num("image_s"), ret: SlotReturn::from_value(e.get("retour")) }
                }
                "navigation" => {
                    let action = st("action").and_then(NavAction::from_name).ok_or_else(|| trf!("item {}: unknown action", k + 1))?;
                    let target = match e.get("cible") {
                        Some(c) if action == NavAction::Goto => Some(Target::from_value(c).ok_or_else(|| trf!("item {}: invalid target", k + 1))?),
                        _ => None,
                    };
                    Kind::Nav { action, text: st("texte").unwrap_or(action.default_text()).to_string(), target }
                }
                "texte" => {
                    let c = st("couleur").unwrap_or("#ffffff").trim_start_matches('#');
                    let rgb = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| trf!("item {}: invalid color", k + 1))?;
                    Kind::Text { text: st("texte").unwrap_or("").to_string(), size: at("taille")?, color: [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8],
                                 align: st("alignement").and_then(Align::from_name).unwrap_or(Align::Center), font: st("police").unwrap_or("").to_string(), link: link()?,
                                 deco: TextDeco::from_item(e).map_err(|m| trf!("item {}: {}", k + 1, m))? }
                }
                "image" if e.get("instant").is_some() || e.get("video").is_some() => {
                    let video = e.get("video").and_then(Value::as_f64).filter(|v| *v >= 1.0).map(|v| v as usize - 1);
                    let at = e.get("instant").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
                    let crop = e.get("recadrage").and_then(Value::as_array).filter(|a| a.len() == 4).and_then(|a| {
                        let v: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
                        (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| [v[0].clamp(0.0, 1.0), v[1].clamp(0.0, 1.0), v[2].min(1.0), v[3].min(1.0)])
                    });
                    Kind::Image { path: String::new(), link: link()?, frame: Some(VideoFrame { video, at, crop }) }
                }
                "forme" => Kind::Shape {
                    shape: st("forme").and_then(ShapeKind::from_name).unwrap_or(ShapeKind::Rect),
                    paint: Paint::from_item(e).map_err(|m| trf!("item {}: {}", k + 1, m))?,
                },
                "image" => Kind::Image { path: st("fichier").ok_or_else(|| trf!("item {}: “fichier” missing", k + 1))?.to_string(), link: link()?, frame: None },
                t => return Err(trf!("item {}: unknown type “{}”", k + 1, t)),
            };
            let look = Look {
                style: match e.get("style") { Some(v @ Value::Obj(_)) => Some(ButtonStyle::from_value(v).map_err(|m| trf!("item {}: {}", k + 1, m))?), _ => None },
                skin: e.get("images_etats").map(Skin::from_value).unwrap_or_default(),
                sounds: e.get("sons").map(crate::style::ItemSounds::from_value),
            };
            let layer = e.get("calque").and_then(Value::as_f64).map_or(0, |x| x.max(0.0) as u32);
            items.push(Item { x: at("x")?, y: at("y")?, w: at("w")?, h: at("h")?, kind, look, layer, sub: SubChoice::from_value(e.get("soustitres")), id: e.get("id").and_then(Value::as_f64).map_or(0, |x| x.max(0.0) as u32), nav: NavLinks::from_value(e.get("navigation")), name: e.get("nom").and_then(Value::as_str).unwrap_or("").to_string() });
        }
        let layers = v.get("calques").and_then(Value::as_array).map(|a| a.iter().enumerate().map(|(k, l)| Layer {
            id: l.get("id").and_then(Value::as_f64).map_or(k as u32 + 1, |x| x.max(1.0) as u32),
            name: l.get("nom").and_then(Value::as_str).unwrap_or("").to_string(),
            hidden: l.get("masque").and_then(Value::as_bool).unwrap_or(false),
            locked: l.get("verrouille").and_then(Value::as_bool).unwrap_or(false),
        }).collect()).unwrap_or_default();
        let mut d = PageDesign { items, layers, name: v.get("nom").and_then(Value::as_str).unwrap_or("").to_string(), bg: v.get("fond_page").map(PageBg::from_value).unwrap_or_default(),
            anim_fps: v.get("animation_ips").and_then(Value::as_f64).map_or(0, |x| x.round().clamp(0.0, 60.0) as u32),
            idle: v.get("lancement_auto").and_then(|a| Some(Idle {
                seconds: a.get("secondes").and_then(Value::as_f64)?.round().clamp(1.0, MAX_IDLE_SECONDS as f64) as u32,
                item: a.get("element").and_then(Value::as_f64)?.max(0.0) as u32,
            })) };
        d.normalize_layers();
        d.normalize_ids();
        Ok(d)
    }

    /// Identifiants d'elements : les doublons (fichier modifie a la main) sont remis a zero.
    pub fn normalize_ids(&mut self) {
        let mut seen = std::collections::HashSet::new();
        for it in &mut self.items {
            if it.id != 0 && !seen.insert(it.id) { it.id = 0; }
        }
    }

    /// Identifiant de l'element `i` (attribue au besoin : le plus grand + 1).
    pub fn ensure_id(&mut self, i: usize) -> u32 {
        if self.items[i].id == 0 {
            let next = self.items.iter().map(|it| it.id).max().unwrap_or(0) + 1;
            self.items[i].id = next;
        }
        self.items[i].id
    }

    /// Rang de l'element portant l'identifiant `id`.
    pub fn item_by_id(&self, id: u32) -> Option<usize> {
        if id == 0 { return None; }
        self.items.iter().position(|it| it.id == id)
    }

    /// Oublie les liens de navigation qui designent un element disparu de la page (ils redeviennent automatiques).
    pub fn prune_nav(&mut self) {
        let ids: std::collections::HashSet<u32> = self.items.iter().map(|it| it.id).filter(|i| *i != 0).collect();
        for it in &mut self.items {
            for d in 0..4 {
                if let NavLink::Item(id) = it.nav.get(d) { if !ids.contains(&id) { it.nav.set(d, NavLink::Auto); } }
            }
        }
    }
}

/// Calque de l'editeur : un groupe d'elements d'une page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layer {
    /// identifiant stable (>= 1) reference par `Item::layer`
    pub id: u32,
    pub name: String,
    /// masque dans l'apercu de l'editeur (reste sur le disque)
    pub hidden: bool,
    /// verrouille : ni selection ni deplacement a la souris dans l'apercu
    pub locked: bool,
}

/// Mises en page personnalisees du projet. Une page absente garde la grille automatique.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Designs {
    /// Toutes les pages du menu, dans l'ordre (page 0 = page 1 du menu). Leurs vignettes montrent n'importe quelle video du projet.
    pub free: Vec<PageDesign>,
    /// Apparence des boutons de tout le projet (chaque element peut avoir la sienne, voir `Item::look`).
    pub style: ButtonStyle,
    /// Page d'accueil (touche TOP MENU et ouverture du disque) ; `None` = page 1.
    pub home: Option<Target>,
}

impl Designs {
    pub fn is_empty(&self) -> bool { self.free.is_empty() && self.style == ButtonStyle::default() && self.home.is_none() }

    /// Pages du menu.
    pub fn pages(&self) -> &[PageDesign] { &self.free }

    /// Echange deux videos (vignettes et destinations suivent leur video).
    /// Aligne l'instant des vignettes liees a un chapitre sur celui du chapitre (`lookup(video, id)` : instant, s'il existe).
    /// Une vignette dont le chapitre n'existe plus (ou dont la video a change) devient une vignette a instant libre : elle
    /// garde son instant. Rend vrai si quelque chose a change.
    pub fn sync_chapters(&mut self, lookup: &dyn Fn(usize, u32) -> Option<f64>) -> bool {
        let mut changed = false;
        for d in &mut self.free {
            for it in &mut d.items {
                if let Kind::Slot { video, start, chapter, .. } = &mut it.kind {
                    let Some(id) = *chapter else { continue };
                    match video.and_then(|v| lookup(v, id)) {
                        Some(t) => if *start != Some(t) { *start = Some(t); changed = true; },
                        None => { *chapter = None; changed = true; }
                    }
                }
            }
        }
        changed
    }

    /// Chapitres (video, identifiant) utilises par des vignettes.
    pub fn linked_chapters(&self) -> std::collections::BTreeSet<(usize, u32)> {
        self.free.iter().flat_map(|d| d.items.iter()).filter_map(|it| match it.kind {
            Kind::Slot { video: Some(v), chapter: Some(c), start: Some(_), .. } => Some((v, c)),
            _ => None,
        }).collect()
    }

    /// Nombre de vignettes liees au chapitre `id` de la video `video`.
    pub fn chapter_uses(&self, video: usize, id: u32) -> usize {
        self.free.iter().flat_map(|d| d.items.iter()).filter(|it| matches!(it.kind, Kind::Slot { video: Some(v), chapter: Some(c), .. } if v == video && c == id)).count()
    }

    pub fn swap_videos(&mut self, a: usize, b: usize) {
        for d in &mut self.free {
            for it in &mut d.items {
                if let Kind::Slot { video: Some(v), .. } | Kind::Image { frame: Some(VideoFrame { video: Some(v), .. }), .. } = &mut it.kind { *v = if *v == a { b } else if *v == b { a } else { *v }; }
                if let Some(Some(t)) = it.targets_mut() { t.swap_videos(a, b); }
            }
        }
        if let Some(h) = &mut self.home { h.swap_videos(a, b); }
    }

    /// La page `page` vient d'etre supprimee : les boutons qui y menaient perdent leur cible, les suivants sont decales.
    pub fn page_removed(&mut self, page: usize) {
        for d in &mut self.free {
            for it in &mut d.items {
                if let Kind::Slot { ret, .. } = &mut it.kind {
                    match *ret {
                        SlotReturn::Page(p) if p == page => *ret = SlotReturn::Origin,
                        SlotReturn::Page(p) if p > page => *ret = SlotReturn::Page(p - 1),
                        _ => {}
                    }
                }
                if let Some(target) = it.targets_mut() {
                    if let Some(t) = target { if !t.page_removed(page) { *target = None; } }
                }
            }
        }
        match self.home {
            Some(Target::Main(p)) if p == page => self.home = None,
            Some(Target::Main(p)) if p > page => self.home = Some(Target::Main(p - 1)),
            _ => {}
        }
    }

    /// Retire une video : ses vignettes restent en place sans video, les destinations vers elle sont effacees, les suivantes renumerotees.
    pub fn remove_video(&mut self, i: usize) {
        for d in &mut self.free {
            for it in &mut d.items {
                let video = match &mut it.kind {
                    Kind::Slot { video, .. } => Some(video),
                    Kind::Image { frame: Some(f), .. } => Some(&mut f.video),
                    _ => None,
                };
                if let Some(video) = video {
                    *video = match *video { Some(v) if v == i => None, Some(v) if v > i => Some(v - 1), v => v };
                }
                if let Some(target) = it.targets_mut() {
                    match *target {
                        Some(Target::Video(v)) if v == i => *target = None,
                        Some(Target::Video(v)) if v > i => *target = Some(Target::Video(v - 1)),
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn to_value(&self) -> Value {
        let mut o = vec![("pages".to_string(), Value::Arr(self.free.iter().map(PageDesign::to_value).collect()))];
        if self.style != ButtonStyle::default() { o.push(("style_boutons".into(), self.style.to_value())); }
        if let Some(h) = self.home { o.push(("accueil".into(), h.to_value())); }
        Value::Obj(o)
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let mut d = Designs::default();
        if let Some(pages) = v.get("pages").and_then(Value::as_array) {
            for (k, page) in pages.iter().enumerate() {
                d.free.push(PageDesign::from_value(page).map_err(|e| trf!("page {}: {}", k + 1, e))?);
            }
        }
        if let Some(h) = v.get("accueil") { d.home = Some(Target::from_value(h).ok_or(tr("invalid home page"))?); }
        if let Some(st) = v.get("style_boutons") { d.style = ButtonStyle::from_value(st).map_err(|e| trf!("button style: {}", e))?; }
        Ok(d)
    }
}
