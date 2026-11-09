//! Lecteur JSON minimal (RFC 8259) sans dependance, pour les fichiers de projet.
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonError(pub String);
impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "JSON invalide : {}", self.0) }
}
impl std::error::Error for JsonError {}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self { Value::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v), _ => None }
    }
    pub fn as_str(&self) -> Option<&str> { if let Value::Str(s) = self { Some(s) } else { None } }
    pub fn as_f64(&self) -> Option<f64> { if let Value::Num(n) = self { Some(*n) } else { None } }
    pub fn as_i64(&self) -> Option<i64> { self.as_f64().filter(|n| n.fract() == 0.0).map(|n| n as i64) }
    pub fn as_bool(&self) -> Option<bool> { if let Value::Bool(b) = self { Some(*b) } else { None } }
    pub fn as_array(&self) -> Option<&[Value]> { if let Value::Arr(a) = self { Some(a) } else { None } }
    pub fn is_null(&self) -> bool { matches!(self, Value::Null) }
}

struct P<'a> { s: &'a [u8], i: usize }

pub fn parse(text: &str) -> Result<Value, JsonError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = P { s: text.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() { return p.fail("texte apres la valeur"); }
    Ok(v)
}

impl<'a> P<'a> {
    fn fail<T>(&self, m: &str) -> Result<T, JsonError> {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&b| b == b'\n').count() + 1;
        Err(JsonError(format!("{m} (ligne {line}, octet {})", self.i)))
    }
    fn ws(&mut self) { while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') { self.i += 1; } }
    fn peek(&self) -> Option<u8> { self.s.get(self.i).copied() }
    fn lit(&mut self, w: &str, v: Value) -> Result<Value, JsonError> {
        if self.s[self.i..].starts_with(w.as_bytes()) { self.i += w.len(); Ok(v) } else { self.fail("mot inconnu") }
    }
    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > 200 { return self.fail("imbrication trop profonde"); }
        match self.peek() {
            None => self.fail("fin inattendue"),
            Some(b'n') => self.lit("null", Value::Null),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.peek() == Some(b']') { self.i += 1; return Ok(Value::Arr(a)); }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => { self.i += 1; return Ok(Value::Arr(a)); }
                        _ => return self.fail("',' ou ']' attendu"),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut o: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') { self.i += 1; return Ok(Value::Obj(o)); }
                loop {
                    self.ws();
                    if self.peek() != Some(b'"') { return self.fail("cle entre guillemets attendue"); }
                    let k = self.string()?;
                    self.ws();
                    if self.peek() != Some(b':') { return self.fail("':' attendu"); }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    if let Some(e) = o.iter_mut().find(|(kk, _)| *kk == k) { e.1 = v } else { o.push((k, v)) } // la derniere valeur gagne (comme Python)
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => { self.i += 1; return Ok(Value::Obj(o)); }
                        _ => return self.fail("',' ou '}' attendu"),
                    }
                }
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => {
                let st = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') { self.i += 1; }
                std::str::from_utf8(&self.s[st..self.i]).ok().and_then(|t| t.parse::<f64>().ok()).map(Value::Num).ok_or(()).or_else(|_| self.fail("nombre invalide"))
            }
            Some(_) => self.fail("caractere inattendu"),
        }
    }
    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.i + 4 > self.s.len() { return self.fail("sequence \\u tronquee"); }
        let t = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap_or("");
        let v = u32::from_str_radix(t, 16).or_else(|_| self.fail("sequence \\u invalide"))?;
        self.i += 4;
        Ok(v)
    }
    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1; // guillemet ouvrant
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return self.fail("chaine non terminee") };
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).or_else(|_| self.fail("UTF-8 invalide")),
                b'\\' => {
                    let Some(e) = self.peek() else { return self.fail("echappement tronque") };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"', b'\\' => '\\', b'/' => '/', b'b' => '\u{8}', b'f' => '\u{c}', b'n' => '\n', b'r' => '\r', b't' => '\t',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + lo.wrapping_sub(0xDC00);
                            }
                            char::from_u32(cp).unwrap_or('\u{fffd}')
                        }
                        _ => return self.fail("echappement inconnu"),
                    };
                    let mut b = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                c if c < 0x20 => return self.fail("caractere de controle dans une chaine"),
                c => out.push(c),
            }
        }
    }
}

// ------------------------------------------------------------------ ecriture
fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn num(n: f64, out: &mut String) {
    if n.fract() == 0.0 && n.abs() < 1e15 { out.push_str(&format!("{}", n as i64)) } else { out.push_str(&format!("{n}")) }
}

impl Value {
    /// JSON indente de 2 espaces, UTF-8 non echappe (lisible et modifiable a la main).
    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0);
        s.push('\n');
        s
    }
    fn write(&self, out: &mut String, depth: usize) {
        let pad = |out: &mut String, d: usize| out.push_str(&"  ".repeat(d));
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => num(*n, out),
            Value::Str(s) => esc(s, out),
            Value::Arr(a) if a.is_empty() => out.push_str("[]"),
            Value::Arr(a) => {
                out.push_str("[\n");
                for (i, v) in a.iter().enumerate() {
                    pad(out, depth + 1);
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push(']');
            }
            Value::Obj(o) if o.is_empty() => out.push_str("{}"),
            Value::Obj(o) => {
                out.push_str("{\n");
                for (i, (k, v)) in o.iter().enumerate() {
                    pad(out, depth + 1);
                    esc(k, out);
                    out.push_str(": ");
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push('}');
            }
        }
    }
}

/// Cles du fichier de projet : (nom interne historique, nom ecrit dans le fichier). Le code manipule les noms internes ; le
/// fichier, lui, est en anglais. A la lecture les deux formes sont acceptees (un ancien fichier en francais reste lisible) ;
/// une cle absente de la table (cle inconnue, deja anglaise) traverse sans changement.
const PROJECT_KEYS: &[(&str, &str)] = &[
    ("titre", "title"),
    ("langue", "language"),
    ("encodage", "encoding"),
    ("mise_en_page", "layout"),
    ("langues", "languages"),
    ("jaquette", "cover"),
    ("etiquette", "label"),
    ("fichier", "file"),
    ("vignette_s", "thumbnail_s"),
    ("chapitres", "chapters"),
    ("chapitres_auto_s", "auto_chapters_s"),
    ("chapitres_exacts", "exact_chapters"),
    ("soustitres", "subtitles"),
    ("pistes_audio", "audio_tracks"),
    ("remplir_disque", "fill_disc"),
    ("copie_si_conforme", "copy_if_conforming"),
    ("fond", "background"),
    ("fond_video", "background_video"),
    ("son_fond_video", "background_video_audio"),
    ("musique", "music"),
    ("fond_secondes", "background_seconds"),
    ("taille", "size"),
    ("boucle", "loop"),
    ("cache", "use_cache"),
    ("calque", "layer"),
    ("calques", "layers"),
    ("elements", "items"),
    ("nom", "name"),
    ("verrouille", "locked"),
    ("masque", "hidden"),
    ("texte", "text"),
    ("police", "font"),
    ("couleur", "color"),
    ("couleurs", "colors"),
    ("alignement", "align"),
    ("alignement_v", "valign"),
    ("cible", "target"),
    ("lire", "play"),
    ("haut", "up"),
    ("bas", "down"),
    ("gauche", "left"),
    ("droite", "right"),
    ("chapitre", "chapter"),
    ("depart_s", "start_s"),
    ("libelle", "caption"),
    ("vignette_h", "thumbnail_h"),
    ("retour", "return_to"),
    ("ensuite", "then"),
    ("rester", "stay"),
    ("secondes", "seconds"),
    ("element", "item"),
    ("lancement_auto", "auto_launch"),
    ("opacite", "opacity"),
    ("son_video", "video_sound"),
    ("sons", "sounds"),
    ("style_boutons", "button_style"),
    ("images_etats", "state_images"),
    ("fond_page", "page_background"),
    ("animation_ips", "animation_fps"),
    ("accueil", "home"),
    ("contour", "outline"),
    ("contour_couleur", "outline_color"),
    ("contour_mm", "outline_mm"),
    ("fond_arrondi", "background_radius"),
    ("fond_couleur", "background_color"),
    ("fond_opacite", "background_opacity"),
    ("fond_marge", "background_margin"),
    ("fond_cliquable", "background_clickable"),
    ("bordure", "border"),
    ("arrondi", "radius"),
    ("forme", "shape"),
    ("trait", "line_style"),
    ("epaisseur", "thickness"),
    ("epaisseur_mm", "thickness_mm"),
    ("fleche", "arrow"),
    ("remplissage", "fill"),
    ("recadrage", "crop"),
    ("instant", "time_s"),
    ("retourne", "flip"),
    ("inverse", "flipped"),
    ("audio_langue", "audio_language"),
    ("langue_id", "language_id"),
    ("piste", "track"),
    ("agrandir", "zoom"),
    ("assombrir", "dim"),
    ("animation_selection", "animation_selected"),
    ("animation_une_fois", "animation_once"),
    ("animation_normal_une_fois", "animation_normal_once"),
    ("cadre", "frame"),
    ("cadre_active", "frame_activated"),
    ("cadre_epaisseur", "frame_thickness"),
    ("cadre_selection", "frame_selected"),
    ("cadre_selection_epaisseur", "frame_selected_thickness"),
    ("etat_active", "activated_state"),
    ("fond_active", "background_activated"),
    ("fond_selection", "background_selected"),
    ("texte_active", "text_activated"),
    ("texte_selection", "text_selected"),
    ("halo", "glow"),
    ("halo_anime", "glow_animated"),
    ("halo_couleur", "glow_color"),
    ("souligner", "underline"),
    ("selectionne", "selected"),
    ("active", "activated"),
    ("selection", "select"),
    ("activation", "activate"),
    ("son_selection", "sound_select"),
    ("son_activation", "sound_activate"),
    ("ajuster", "autofit"),
    ("gras", "bold"),
    ("italique", "italic"),
    ("interligne", "line_spacing"),
    ("ombre", "shadow"),
    ("ombre_mm", "shadow_mm"),
    ("ombre_couleur", "shadow_color"),
    ("ombre_opacite", "shadow_opacity"),
    ("taille_pt", "size_pt"),
    ("image_fond", "background_image"),
    ("image_video", "video_image"),
    ("reperes", "marks"),
    ("face_mm", "face_width_mm"),
    ("hauteur_mm", "height_mm"),
    ("tranche_mm", "spine_mm"),
    ("fond_perdu_mm", "bleed_mm"),
    ("traits_coupe", "crop_marks"),
    ("dos", "back"),
    ("tranche", "spine"),
    ("face", "front"),
    ("diametre_mm", "diameter_mm"),
    ("trou_mm", "hole_mm"),
    ("rayon_mm", "radius_mm"),
    ("aligner", "align_blocks"),
    ("capacite", "capacity"),
    ("compatibilite", "compatibility"),
    ("conserver", "keep"),
    ("date_fixe", "fixed_date"),
    ("miroir", "mirror"),
    ("nom_volume", "volume_name"),
    ("ordre_lecture", "play_order"),
    ("organisation", "organization"),
    ("proprietaire", "owner"),
    ("verifier", "verify"),
    ("marge", "margin"),
    ("marge_cote", "side_margin"),
    ("cadence_ips", "fps"),
    ("cesure", "wrap"),
    ("decalage_ms", "offset_ms"),
    ("largeur_max", "max_width"),
    ("mise_en_forme", "keep_formatting"),
    ("positions_fichier", "file_positions"),
    ("retirer_indications", "strip_notes"),
];

fn rename_keys(v: &Value, to_file: bool) -> Value {
    match v {
        Value::Arr(a) => Value::Arr(a.iter().map(|x| rename_keys(x, to_file)).collect()),
        Value::Obj(o) => Value::Obj(o.iter().map(|(k, x)| {
            let name = PROJECT_KEYS.iter().find(|(i, f)| if to_file { i == k } else { f == k }).map(|(i, f)| if to_file { *f } else { *i });
            (name.unwrap_or(k).to_string(), rename_keys(x, to_file))
        }).collect()),
        other => other.clone(),
    }
}

/// Arbre du projet avec les noms de cles du fichier (anglais).
pub fn project_to_file(v: &Value) -> Value { rename_keys(v, true) }

/// Lit un fichier de projet (cles anglaises ou anciennes cles francaises) et rend l'arbre aux noms internes.
pub fn parse_project(text: &str) -> Result<Value, JsonError> { parse(text).map(|v| rename_keys(&v, false)) }
