//! Apparence des boutons de menu : couleurs et cadre des etats normal / selectionne / active, halo, agrandissement, assombrissement,
//! images d'etats personnalisees ; estimation du poids d'une page de menu.
//!
//! Les tailles (epaisseurs, arrondi, halo) sont exprimees sur le plan de reference 1920x1080 et mises a l'echelle du menu.
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
}

impl Default for ButtonStyle {
    fn default() -> Self {
        ButtonStyle {
            normal_bg: [24, 32, 58], normal_text: [255, 255, 255], normal_border: [70, 85, 120], normal_border_w: 1, dim: 0,
            sel_bg: [60, 100, 190], sel_text: [255, 255, 255], sel_border: [255, 255, 255], sel_border_w: 5, radius: 0,
            glow: 0, glow_color: [120, 180, 255], zoom: 0, backdrop: 60, underline: false,
            activated: false, act_bg: [240, 150, 40], act_text: [255, 255, 255], act_border: [255, 230, 150],
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

    /// Nombre d'images differentes par bouton.
    pub fn state_count(&self) -> u64 {
        if self.activated { 3 } else { 2 }
    }

    pub fn to_value(&self) -> Value {
        let hex = |c: [u8; 3]| Value::Str(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
        let n = |v: i32| Value::Num(v as f64);
        Value::Obj(vec![
            ("fond".into(), hex(self.normal_bg)), ("texte".into(), hex(self.normal_text)), ("cadre".into(), hex(self.normal_border)),
            ("cadre_epaisseur".into(), n(self.normal_border_w)), ("assombrir".into(), n(self.dim)),
            ("fond_selection".into(), hex(self.sel_bg)), ("texte_selection".into(), hex(self.sel_text)), ("cadre_selection".into(), hex(self.sel_border)),
            ("cadre_selection_epaisseur".into(), n(self.sel_border_w)), ("arrondi".into(), n(self.radius)),
            ("halo".into(), n(self.glow)), ("halo_couleur".into(), hex(self.glow_color)), ("agrandir".into(), n(self.zoom)),
            ("fond_cliquable".into(), n(self.backdrop)), ("souligner".into(), Value::Bool(self.underline)),
            ("etat_active".into(), Value::Bool(self.activated)), ("fond_active".into(), hex(self.act_bg)),
            ("texte_active".into(), hex(self.act_text)), ("cadre_active".into(), hex(self.act_border)),
        ])
    }

    /// Lecture tolerante : les champs absents gardent leur valeur par defaut.
    pub fn from_value(v: &Value) -> Result<ButtonStyle, String> {
        let mut s = ButtonStyle::default();
        let col = |k: &str, d: &mut [u8; 3]| -> Result<(), String> {
            if let Some(x) = v.get(k).and_then(Value::as_str) {
                let c = x.trim().trim_start_matches('#');
                let n = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| format!("style : couleur « {k} » invalide"))?;
                *d = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            }
            Ok(())
        };
        let int = |k: &str, d: &mut i32| { if let Some(x) = v.get(k).and_then(Value::as_f64) { *d = x.round() as i32; } };
        let boolean = |k: &str, d: &mut bool| { if let Some(x) = v.get(k).and_then(Value::as_bool) { *d = x; } };
        col("fond", &mut s.normal_bg)?;
        col("texte", &mut s.normal_text)?;
        col("cadre", &mut s.normal_border)?;
        int("cadre_epaisseur", &mut s.normal_border_w);
        int("assombrir", &mut s.dim);
        col("fond_selection", &mut s.sel_bg)?;
        col("texte_selection", &mut s.sel_text)?;
        col("cadre_selection", &mut s.sel_border)?;
        int("cadre_selection_epaisseur", &mut s.sel_border_w);
        int("arrondi", &mut s.radius);
        int("halo", &mut s.glow);
        col("halo_couleur", &mut s.glow_color)?;
        int("agrandir", &mut s.zoom);
        int("fond_cliquable", &mut s.backdrop);
        boolean("souligner", &mut s.underline);
        boolean("etat_active", &mut s.activated);
        col("fond_active", &mut s.act_bg)?;
        col("texte_active", &mut s.act_text)?;
        col("cadre_active", &mut s.act_border)?;
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
}

impl Skin {
    pub fn is_empty(&self) -> bool {
        self.normal.is_empty() && self.selected.is_empty() && self.activated.is_empty()
    }
    pub fn get(&self, s: BtnState) -> &str {
        match s { BtnState::Normal => &self.normal, BtnState::Selected => &self.selected, BtnState::Activated => &self.activated }
    }
    pub fn to_value(&self) -> Value {
        let mut o = Vec::new();
        for (k, v) in [("normal", &self.normal), ("selectionne", &self.selected), ("active", &self.activated)] {
            if !v.is_empty() { o.push((k.to_string(), Value::Str(v.clone()))); }
        }
        Value::Obj(o)
    }
    pub fn from_value(v: &Value) -> Skin {
        let g = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Skin { normal: g("normal"), selected: g("selectionne"), activated: g("active") }
    }
}

/// Apparence propre a un element (bouton) : style qui remplace celui du projet, images d'etats.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Look {
    pub style: Option<ButtonStyle>,
    pub skin: Skin,
}

impl Look {
    pub fn is_default(&self) -> bool { self.style.is_none() && self.skin.is_empty() }
}

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
