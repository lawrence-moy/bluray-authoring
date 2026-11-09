//! Apparence des boutons de menu : couleurs et cadre des etats normal / selectionne / active, halo, agrandissement, assombrissement,
//! images d'etats personnalisees ; estimation du poids d'une page de menu.
//!
//! Les tailles (epaisseurs, arrondi, halo) sont exprimees sur le plan de reference 1920x1080 et mises a l'echelle du menu.
use crate::trf;
use crate::json::Value;

/// Etat d'un bouton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BtnState {
    Normal,
    Selected,
    Activated,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ButtonStyle {
    // etat normal
    pub normal_bg: [u8; 3],
    pub normal_text: [u8; 3],
    pub normal_border: [u8; 3],
    pub normal_border_w: i32,
    /// Assombrissement des boutons non selectionnes, en % (0 = aucun, 80 au plus).
    pub dim: i32,
    // etat selectionne
    pub sel_bg: [u8; 3],
    pub sel_text: [u8; 3],
    pub sel_border: [u8; 3],
    pub sel_border_w: i32,
    /// Arrondi des coins (px).
    pub radius: i32,
    /// Rayon du halo autour du bouton selectionne (px, 0 = aucun).
    pub glow: i32,
    pub glow_color: [u8; 3],
    /// Halo anime : il pulse (grandit et diminue en boucle) tant que le bouton est selectionne (`PULSE_FRAMES` images).
    pub glow_pulse: bool,
    /// Agrandissement du bouton selectionne, en % (0 a 20).
    pub zoom: i32,
    /// Textes et images cliquables : opacite (%) du fond colore quand ils sont selectionnes (0 = pas de fond).
    pub backdrop: i32,
    /// Textes et images cliquables : soulignement quand ils sont selectionnes.
    pub underline: bool,
    /// Etat « active » (appui sur OK) distinct ; sinon il reprend l'etat selectionne.
    pub activated: bool,
    pub act_bg: [u8; 3],
    pub act_text: [u8; 3],
    pub act_border: [u8; 3],
    /// Son joue quand la selection arrive sur le bouton (fichier audio ; vide = aucun).
    pub sound_select: String,
    /// Son joue a l'appui sur OK (fichier audio ; vide = aucun).
    pub sound_activate: String,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        ButtonStyle {
            normal_bg: [24, 32, 58], normal_text: [255, 255, 255], normal_border: [70, 85, 120], normal_border_w: 1, dim: 0,
            sel_bg: [60, 100, 190], sel_text: [255, 255, 255], sel_border: [255, 255, 255], sel_border_w: 5, radius: 0,
            glow: 0, glow_color: [120, 180, 255], glow_pulse: false, zoom: 0, backdrop: 60, underline: false,
            activated: false, act_bg: [240, 150, 40], act_text: [255, 255, 255], act_border: [255, 230, 150],
            sound_select: String::new(), sound_activate: String::new(),
        }
    }
}

impl ButtonStyle {
    /// Couleurs d'un etat : (fond, texte, cadre, epaisseur du cadre).
    pub fn colors(&self, s: BtnState) -> ([u8; 3], [u8; 3], [u8; 3], i32) {
        match s {
            BtnState::Normal => (self.normal_bg, self.normal_text, self.normal_border, self.normal_border_w),
            BtnState::Selected => (self.sel_bg, self.sel_text, self.sel_border, self.sel_border_w),
            BtnState::Activated if self.activated => (self.act_bg, self.act_text, self.act_border, self.sel_border_w),
            BtnState::Activated => (self.sel_bg, self.sel_text, self.sel_border, self.sel_border_w),
        }
    }

    /// Valeurs bornees aux plages admises.
    pub fn clamped(&self) -> ButtonStyle {
        let mut s = self.clone();
        s.normal_border_w = s.normal_border_w.clamp(0, 40);
        s.sel_border_w = s.sel_border_w.clamp(0, 40);
        s.dim = s.dim.clamp(0, 80);
        s.radius = s.radius.clamp(0, 200);
        s.glow = s.glow.clamp(0, 60);
        s.zoom = s.zoom.clamp(0, 20);
        s.backdrop = s.backdrop.clamp(0, 100);
        s
    }

    /// Style a l'echelle `k` (menu / plan 1920) : epaisseurs, arrondi et halo ; une epaisseur non nulle reste d'au moins 1 px.
    pub fn scaled(&self, k: f64) -> ButtonStyle {
        let sc = |v: i32| if v <= 0 { 0 } else { ((v as f64 * k).round() as i32).max(1) };
        let mut s = self.clamped();
        s.normal_border_w = sc(s.normal_border_w);
        s.sel_border_w = sc(s.sel_border_w);
        s.radius = sc(s.radius);
        s.glow = sc(s.glow);
        s
    }

    /// Marges (horizontale, verticale) a ajouter de chaque cote de l'image d'un bouton de `w` x `h` (halo et agrandissement).
    pub fn margins(&self, w: i32, h: i32) -> (i32, i32) {
        let z = |d: i32| (d.max(0) * self.zoom.clamp(0, 20) + 199) / 200;
        (self.glow.max(0) + z(w), self.glow.max(0) + z(h))
    }

    /// Nombre d'images differentes par bouton (images du halo anime comprises).
    pub fn state_count(&self) -> u64 {
        (if self.activated { 3 } else { 2 }) + self.pulse_frames() as u64 - 1
    }

    /// Nombre d'images de l'etat selectionne : `PULSE_FRAMES` si le halo est anime (et non nul), sinon 1.
    pub fn pulse_frames(&self) -> usize {
        if self.glow_pulse && self.glow > 0 { PULSE_FRAMES } else { 1 }
    }

    /// Style de l'image `i` (0 = halo complet) de l'animation du halo : rayon entre 35 % et 100 %, variation en cosinus
    /// (boucle sans a-coup). Les marges restent celles du style complet : les images ont toutes la meme taille.
    pub fn pulse_style(&self, i: usize) -> ButtonStyle {
        let mut s = self.clone();
        let t = (1.0 + (2.0 * std::f64::consts::PI * i as f64 / PULSE_FRAMES as f64).cos()) / 2.0;
        s.glow = ((self.glow as f64) * (0.35 + 0.65 * t)).round() as i32;
        s.glow_pulse = false;
        s
    }

    pub fn to_value(&self) -> Value {
        let hex = |c: [u8; 3]| Value::Str(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
        let n = |v: i32| Value::Num(v as f64);
        let mut o = vec![
            ("background".into(), hex(self.normal_bg)), ("text".into(), hex(self.normal_text)), ("frame".into(), hex(self.normal_border)),
            ("frame_thickness".into(), n(self.normal_border_w)), ("dim".into(), n(self.dim)),
            ("background_selected".into(), hex(self.sel_bg)), ("text_selected".into(), hex(self.sel_text)), ("frame_selected".into(), hex(self.sel_border)),
            ("frame_selected_thickness".into(), n(self.sel_border_w)), ("radius".into(), n(self.radius)),
            ("glow".into(), n(self.glow)), ("glow_color".into(), hex(self.glow_color)), ("glow_animated".into(), Value::Bool(self.glow_pulse)), ("zoom".into(), n(self.zoom)),
            ("background_clickable".into(), n(self.backdrop)), ("underline".into(), Value::Bool(self.underline)),
            ("activated_state".into(), Value::Bool(self.activated)), ("background_activated".into(), hex(self.act_bg)),
            ("text_activated".into(), hex(self.act_text)), ("frame_activated".into(), hex(self.act_border)),
        ];
        if !self.sound_select.is_empty() { o.push(("sound_select".into(), Value::Str(self.sound_select.clone()))); }
        if !self.sound_activate.is_empty() { o.push(("sound_activate".into(), Value::Str(self.sound_activate.clone()))); }
        Value::Obj(o)
    }

    /// Lecture tolerante : les champs absents gardent leur valeur par defaut.
    pub fn from_value(v: &Value) -> Result<ButtonStyle, String> {
        let mut s = ButtonStyle::default();
        let col = |k: &str, d: &mut [u8; 3]| -> Result<(), String> {
            if let Some(x) = v.get(k).and_then(Value::as_str) {
                let c = x.trim().trim_start_matches('#');
                let n = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| trf!("style: invalid color “{}”", k))?;
                *d = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            }
            Ok(())
        };
        let int = |k: &str, d: &mut i32| { if let Some(x) = v.get(k).and_then(Value::as_f64) { *d = x.round() as i32; } };
        let boolean = |k: &str, d: &mut bool| { if let Some(x) = v.get(k).and_then(Value::as_bool) { *d = x; } };
        col("background", &mut s.normal_bg)?;
        col("text", &mut s.normal_text)?;
        col("frame", &mut s.normal_border)?;
        int("frame_thickness", &mut s.normal_border_w);
        int("dim", &mut s.dim);
        col("background_selected", &mut s.sel_bg)?;
        col("text_selected", &mut s.sel_text)?;
        col("frame_selected", &mut s.sel_border)?;
        int("frame_selected_thickness", &mut s.sel_border_w);
        int("radius", &mut s.radius);
        int("glow", &mut s.glow);
        col("glow_color", &mut s.glow_color)?;
        boolean("glow_animated", &mut s.glow_pulse);
        int("zoom", &mut s.zoom);
        int("background_clickable", &mut s.backdrop);
        boolean("underline", &mut s.underline);
        boolean("activated_state", &mut s.activated);
        col("background_activated", &mut s.act_bg)?;
        col("text_activated", &mut s.act_text)?;
        col("frame_activated", &mut s.act_border)?;
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        s.sound_select = text("sound_select");
        s.sound_activate = text("sound_activate");
        Ok(s.clamped())
    }
}

/// Images d'etats fournies par l'utilisateur (chemins ; vide = image generee). Sans image « selectionne » ou « active », l'etat est
/// derive de l'image normale avec les effets du style.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Skin {
    pub normal: String,
    pub selected: String,
    pub activated: String,
    /// Animation de l'etat selectionne : images jouees dans l'ordre, au moins deux (une seule image va dans `selected`, qui est
    /// alors vide ici, voir `set_selected`) ; exige une image normale (fixe ou animee).
    pub sel_frames: Vec<String>,
    /// Animation jouee une seule fois (arret sur la derniere image) au lieu d'en boucle.
    pub sel_once: bool,
    /// Animation de l'etat normal : au moins deux images (une seule va dans `normal`, voir `set_normal`).
    pub normal_frames: Vec<String>,
    /// Animation de l'etat normal jouee une seule fois.
    pub normal_once: bool,
}

/// Une image = etat fixe (`fixed`), plusieurs = animation (`frames`) ; vide = aucune image.
fn set_files(fixed: &mut String, frames: &mut Vec<String>, once: &mut bool, files: Vec<String>) {
    let mut files: Vec<String> = files.into_iter().filter(|f| !f.is_empty()).collect();
    if files.len() >= 2 {
        fixed.clear();
        *frames = files;
    } else {
        *fixed = files.pop().unwrap_or_default();
        frames.clear();
        *once = false;
    }
}

impl Skin {
    pub fn is_empty(&self) -> bool {
        self.normal.is_empty() && self.selected.is_empty() && self.activated.is_empty() && self.sel_frames.is_empty() && self.normal_frames.is_empty()
    }
    /// Image(s) normale(s) fournie(s) : l'element est alors dessine avec ses images d'etats.
    pub fn has_normal(&self) -> bool { !self.normal.is_empty() || !self.normal_frames.is_empty() }
    /// Image normale de reference (premiere image si l'etat normal est anime) ; vide sans image normale.
    pub fn normal_file(&self) -> &str { self.normal_frames.first().map_or(self.normal.as_str(), |f| f.as_str()) }
    /// Images de l'etat « selectionne » : aucune, une (image fixe) ou plusieurs (animation, dans l'ordre).
    pub fn selected_files(&self) -> Vec<String> {
        if !self.sel_frames.is_empty() { self.sel_frames.clone() } else if self.selected.is_empty() { vec![] } else { vec![self.selected.clone()] }
    }
    /// Images de l'etat normal : aucune, une ou plusieurs (animation).
    pub fn normal_files(&self) -> Vec<String> {
        if !self.normal_frames.is_empty() { self.normal_frames.clone() } else if self.normal.is_empty() { vec![] } else { vec![self.normal.clone()] }
    }
    /// Choisit les images de l'etat « selectionne » : une image = etat fixe (`selected`), plusieurs = animation (`sel_frames`).
    pub fn set_selected(&mut self, files: Vec<String>) {
        set_files(&mut self.selected, &mut self.sel_frames, &mut self.sel_once, files);
    }
    /// Choisit les images de l'etat normal : une image = etat fixe (`normal`), plusieurs = animation (`normal_frames`).
    pub fn set_normal(&mut self, files: Vec<String>) {
        set_files(&mut self.normal, &mut self.normal_frames, &mut self.normal_once, files);
    }
    /// Nombre d'images de l'animation de l'etat selectionne effectivement utilisee (0 sans animation ou sans image normale).
    pub fn anim_frames(&self) -> usize {
        if self.has_normal() { self.sel_frames.len() } else { 0 }
    }
    /// Nombre d'images de l'animation de l'etat normal (0 sans animation).
    pub fn normal_anim_frames(&self) -> usize { self.normal_frames.len() }
    pub fn get(&self, s: BtnState) -> &str {
        match s { BtnState::Normal => self.normal_file(), BtnState::Selected => &self.selected, BtnState::Activated => &self.activated }
    }
    pub fn to_value(&self) -> Value {
        let mut o = Vec::new();
        for (k, v) in [("normal", &self.normal), ("selected", &self.selected), ("activated", &self.activated)] {
            if !v.is_empty() { o.push((k.to_string(), Value::Str(v.clone()))); }
        }
        let list = |f: &[String]| Value::Arr(f.iter().map(|x| Value::Str(x.clone())).collect());
        if !self.normal_frames.is_empty() {
            o.push(("animation_normal".into(), list(&self.normal_frames)));
            if self.normal_once { o.push(("animation_normal_once".into(), Value::Bool(true))); }
        }
        if !self.sel_frames.is_empty() {
            o.push(("animation_selected".into(), list(&self.sel_frames)));
            if self.sel_once { o.push(("animation_once".into(), Value::Bool(true))); }
        }
        Value::Obj(o)
    }
    pub fn from_value(v: &Value) -> Skin {
        let g = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let frames = |k: &str| -> Vec<String> { v.get(k).and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).filter(|f| !f.is_empty()).map(String::from).collect()).unwrap_or_default() };
        let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        let mut s = Skin { normal: g("normal"), selected: g("selected"), activated: g("activated"), ..Default::default() };
        // une animation (si presente) prime sur l'image fixe du meme etat, comme sur le disque ; une animation d'une image est une image fixe
        let sel = frames("animation_selected");
        if !sel.is_empty() { s.set_selected(sel); if s.sel_frames.len() >= 2 { s.sel_once = flag("animation_once"); } }
        let nor = frames("animation_normal");
        if !nor.is_empty() { s.set_normal(nor); if s.normal_frames.len() >= 2 { s.normal_once = flag("animation_normal_once"); } }
        s
    }
}

/// Apparence propre a un element (bouton) : style qui remplace celui du projet, images d'etats.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Look {
    pub style: Option<ButtonStyle>,
    pub skin: Skin,
    /// Sons propres a l'element (independants de l'apparence) ; `None` = sons du style du projet.
    pub sounds: Option<ItemSounds>,
}

impl Look {
    pub fn is_default(&self) -> bool { self.style.is_none() && self.skin.is_empty() && self.sounds.is_none() }
    /// Sons effectifs de l'element : (selection, appui sur OK) ; les siens, sinon ceux du style du projet.
    pub fn sounds<'a>(&'a self, project: &'a ButtonStyle) -> (&'a str, &'a str) {
        match &self.sounds {
            Some(s) => (&s.select, &s.activate),
            None => (&project.sound_select, &project.sound_activate),
        }
    }
}

/// Sons propres a un element : fichiers audio (vide = aucun son pour cet evenement).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemSounds {
    pub select: String,
    pub activate: String,
}

impl ItemSounds {
    pub fn to_value(&self) -> Value {
        let mut o = Vec::new();
        if !self.select.is_empty() { o.push(("select".to_string(), Value::Str(self.select.clone()))); }
        if !self.activate.is_empty() { o.push(("activate".to_string(), Value::Str(self.activate.clone()))); }
        Value::Obj(o)
    }
    pub fn from_value(v: &Value) -> ItemSounds {
        let g = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        ItemSounds { select: g("select"), activate: g("activate") }
    }
}

/// Images de l'animation du halo (etat selectionne) quand `ButtonStyle::glow_pulse` est actif.
pub const PULSE_FRAMES: usize = 8;

/// Memoire des images de boutons qu'un lecteur doit decoder pour une page (d'apres la specification Blu-ray telle que comprise :
/// tampon d'objets decodes de 16 Mio pour les menus IG ; a confirmer).
pub const DECODED_LIMIT: u64 = 16 * 1024 * 1024;
/// Debit de decodage retenu pour estimer le delai d'affichage (octets, soit pixels, par seconde) — meme hypothese que `required_ics_pts`.
pub const DECODE_RATE: f64 = 8_000_000.0;

/// Poids estime d'une page de menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weight {
    /// Octets d'images de boutons decodees (1 octet par pixel et par image differente).
    pub bytes: u64,
    /// Delai approximatif avant affichage du menu (s), plan graphique compris.
    pub seconds: f64,
    pub buttons: usize,
}

impl Weight {
    pub fn ratio(&self) -> f64 { self.bytes as f64 / DECODED_LIMIT as f64 }
}
