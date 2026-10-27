//! Sous-titres texte : lecture de fichiers SRT et ASS/SSA, fusion des repliques simultanees, style d'affichage.
//! Les temps sont en millisecondes depuis le debut de la video.

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Position verticale du texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VPos {
    Bottom,
    Middle,
    Top,
}

impl VPos {
    pub fn name(self) -> &'static str {
        match self { VPos::Bottom => "bas", VPos::Middle => "milieu", VPos::Top => "haut" }
    }
    pub fn from_name(s: &str) -> Option<VPos> {
        match s { "bas" => Some(VPos::Bottom), "milieu" => Some(VPos::Middle), "haut" => Some(VPos::Top), _ => None }
    }
}

/// Style d'une piste de sous-titres. Les tailles sont donnees pour une video de 1080 lignes (mises a l'echelle pour 720).
#[derive(Debug, Clone, PartialEq)]
pub struct SubStyle {
    pub font: String,
    pub size: i32,
    pub color: [u8; 3],
    pub outline_color: [u8; 3],
    pub outline: i32,
    pub pos: VPos,
    pub margin: i32,
}

impl Default for SubStyle {
    fn default() -> Self {
        SubStyle { font: String::new(), size: 56, color: [255, 255, 255], outline_color: [0, 0, 0], outline: 3, pos: VPos::Bottom, margin: 70 }
    }
}

// ------------------------------------------------------------------ decodage du texte
const CP1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š',
    '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

/// Octets d'un fichier texte -> chaine : UTF-8 (avec ou sans BOM), UTF-16 avec BOM, sinon Windows-1252.
pub fn decode_text(b: &[u8]) -> String {
    if b.starts_with(&[0xFF, 0xFE]) || b.starts_with(&[0xFE, 0xFF]) {
        let le = b[0] == 0xFF;
        let u: Vec<u16> = b[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        return String::from_utf16_lossy(&u);
    }
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| if (0x80..0xA0).contains(&c) { CP1252[(c - 0x80) as usize] } else { c as char }).collect(),
    }
}

fn clean_entities(s: &str) -> String {
    s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// Retire les balises `<...>` (HTML) et les blocs `{...}` (ASS) d'une ligne.
fn strip_tags(s: &str) -> String {
    let (mut out, mut depth_a, mut depth_b) = (String::new(), 0usize, 0usize);
    for c in s.chars() {
        match c {
            '<' => depth_a += 1,
            '>' if depth_a > 0 => depth_a -= 1,
            '{' => depth_b += 1,
            '}' if depth_b > 0 => depth_b -= 1,
            _ if depth_a == 0 && depth_b == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

// ------------------------------------------------------------------ SRT
/// « 00:01:02,345 » (ou avec un point, ou des heures a un chiffre) -> millisecondes.
fn parse_srt_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let (hms, frac) = match s.find([',', '.']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let mut it = hms.split(':').map(|p| p.trim().parse::<i64>().ok());
    let (h, m, sec) = (it.next()??, it.next()??, it.next()??);
    if it.next().is_some() || frac.chars().any(|c| !c.is_ascii_digit()) {
        return None;
    }
    let mut f = frac.to_string();
    f.truncate(3);
    while f.len() < 3 {
        f.push('0');
    }
    Some(((h * 60 + m) * 60 + sec) * 1000 + f.parse::<i64>().ok()?)
}

pub fn parse_srt(text: &str) -> Vec<Cue> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    let mut cur: Option<(i64, i64, Vec<String>)> = None;
    let flush = |cur: &mut Option<(i64, i64, Vec<String>)>, cues: &mut Vec<Cue>| {
        if let Some((s, e, lines)) = cur.take() {
            let t = lines.join("\n").trim().to_string();
            if !t.is_empty() && e > s {
                cues.push(Cue { start_ms: s, end_ms: e, text: t });
            }
        }
    };
    for line in text.lines() {
        if let Some(i) = line.find("-->") {
            let (a, b) = (parse_srt_time(&line[..i]), line[i + 3..].split_whitespace().next().and_then(parse_srt_time));
            if let (Some(a), Some(b)) = (a, b) {
                flush(&mut cur, &mut cues);
                cur = Some((a, b, Vec::new()));
                continue;
            }
        }
        if line.trim().is_empty() {
            flush(&mut cur, &mut cues);
        } else if let Some((_, _, lines)) = cur.as_mut() {
            let l = clean_entities(&strip_tags(line));
            if !l.trim().is_empty() {
                lines.push(l.trim().to_string());
            }
        }
    }
    flush(&mut cur, &mut cues);
    cues
}

// ------------------------------------------------------------------ ASS / SSA
/// « 0:01:02.34 » (centiemes) -> millisecondes.
fn parse_ass_time(s: &str) -> Option<i64> {
    parse_srt_time(s)
}

pub fn parse_ass(text: &str) -> Vec<Cue> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let (mut in_events, mut cols): (bool, Vec<String>) = (false, Vec::new());
    let mut cues = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_events = l.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(f) = l.strip_prefix("Format:") {
            cols = f.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
        } else if let Some(d) = l.strip_prefix("Dialogue:") {
            let n = if cols.is_empty() { 10 } else { cols.len() };
            let parts: Vec<&str> = d.splitn(n, ',').collect();
            let col = |name: &str, dflt: usize| cols.iter().position(|c| c == name).unwrap_or(dflt);
            let (is, ie, it) = (col("start", 1), col("end", 2), col("text", n - 1));
            let (Some(a), Some(b), Some(t)) = (parts.get(is).and_then(|x| parse_ass_time(x)), parts.get(ie).and_then(|x| parse_ass_time(x)), parts.get(it)) else {
                continue;
            };
            let t = strip_tags(&t.replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " "));
            let t = t.lines().map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n");
            if !t.is_empty() && b > a {
                cues.push(Cue { start_ms: a, end_ms: b, text: t });
            }
        }
    }
    cues
}

/// Choisit le format d'apres l'extension (`.ass`, `.ssa`, sinon SRT), puis ordonne par debut.
pub fn parse_file_text(ext: &str, text: &str) -> Vec<Cue> {
    let mut c = if matches!(ext.to_ascii_lowercase().as_str(), "ass" | "ssa") { parse_ass(text) } else { parse_srt(text) };
    c.sort_by_key(|x| (x.start_ms, x.end_ms));
    c
}

pub fn load_cues(path: &std::path::Path) -> Result<Vec<Cue>, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let c = parse_file_text(ext, &decode_text(&b));
    if c.is_empty() {
        return Err(format!("{}: aucune replique lisible", path.display()));
    }
    Ok(c)
}

// ------------------------------------------------------------------ fusion
/// Decale tous les temps ; les repliques entierement avant 0 disparaissent, celles qui chevauchent 0 sont tronquees.
pub fn shift(cues: &[Cue], offset_ms: i64) -> Vec<Cue> {
    cues.iter()
        .filter_map(|c| {
            let (s, e) = ((c.start_ms + offset_ms).max(0), c.end_ms + offset_ms);
            (e > s).then(|| Cue { start_ms: s, end_ms: e, text: c.text.clone() })
        })
        .collect()
}

/// Remplace les repliques qui se chevauchent par des intervalles successifs sans recouvrement : pendant un recouvrement,
/// les textes sont affiches ensemble (un par ligne, dans l'ordre de debut). Les intervalles contigus de meme texte sont soudes.
pub fn merge(cues: &[Cue]) -> Vec<Cue> {
    let mut cs: Vec<&Cue> = cues.iter().filter(|c| c.end_ms > c.start_ms && !c.text.trim().is_empty()).collect();
    cs.sort_by_key(|c| (c.start_ms, c.end_ms));
    let mut pts: Vec<i64> = cs.iter().flat_map(|c| [c.start_ms, c.end_ms]).collect();
    pts.sort_unstable();
    pts.dedup();
    let mut out: Vec<Cue> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let t = cs.iter().filter(|c| c.start_ms <= a && c.end_ms >= b).map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n");
        if t.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(l) if l.end_ms == a && l.text == t => l.end_ms = b,
            _ => out.push(Cue { start_ms: a, end_ms: b, text: t }),
        }
    }
    out
}

// ------------------------------------------------------------------ reglages d'une piste (projet)
use crate::json::Value;

/// D'ou viennent les repliques.
#[derive(Debug, Clone, PartialEq)]
pub enum SubSource {
    /// Fichier .srt / .ass / .ssa (chemin absolu ou relatif au projet).
    File(String),
    /// Piste de sous-titres texte integree a la video : numero parmi les pistes de sous-titres (0 = la premiere).
    Embedded(u32),
}

/// Une piste de sous-titres d'une video.
#[derive(Debug, Clone, PartialEq)]
pub struct SubTrackCfg {
    pub source: SubSource,
    /// Code de langue de 3 lettres (ex. fra).
    pub lang: String,
    /// Decalage a appliquer aux temps (ms, peut etre negatif).
    pub offset_ms: i64,
    pub style: SubStyle,
}

impl SubTrackCfg {
    pub fn from_file(path: &str, lang: &str) -> Self {
        SubTrackCfg { source: SubSource::File(path.into()), lang: lang.into(), offset_ms: 0, style: SubStyle::default() }
    }
    pub fn embedded(index: u32, lang: &str) -> Self {
        SubTrackCfg { source: SubSource::Embedded(index), lang: lang.into(), offset_ms: 0, style: SubStyle::default() }
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let source = match (v.get("fichier").and_then(Value::as_str), v.get("piste").and_then(Value::as_i64)) {
            (Some(f), _) => SubSource::File(f.into()),
            (None, Some(n)) if n >= 0 => SubSource::Embedded(n as u32),
            _ => return Err("sous-titres : « fichier » ou « piste » attendu".into()),
        };
        let mut st = SubStyle::default();
        if let Some(s) = v.get("style") {
            if let Some(x) = s.get("police").and_then(Value::as_str) { st.font = x.into(); }
            if let Some(x) = s.get("taille").and_then(Value::as_i64) { st.size = x.clamp(8, 400) as i32; }
            if let Some(x) = s.get("couleur").and_then(Value::as_str) { st.color = parse_hex(x)?; }
            if let Some(x) = s.get("contour_couleur").and_then(Value::as_str) { st.outline_color = parse_hex(x)?; }
            if let Some(x) = s.get("contour").and_then(Value::as_i64) { st.outline = x.clamp(0, 30) as i32; }
            if let Some(x) = s.get("position").and_then(Value::as_str) { st.pos = VPos::from_name(x).ok_or_else(|| format!("position de sous-titre inconnue : {x}"))?; }
            if let Some(x) = s.get("marge").and_then(Value::as_i64) { st.margin = x.clamp(0, 540) as i32; }
        }
        Ok(SubTrackCfg {
            source,
            lang: v.get("langue").and_then(Value::as_str).unwrap_or("und").into(),
            offset_ms: v.get("decalage_ms").and_then(Value::as_f64).map_or(0, |x| x.round() as i64),
            style: st,
        })
    }

    pub fn to_value(&self) -> Value {
        let hex = |c: [u8; 3]| Value::Str(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
        let mut o = vec![match &self.source {
            SubSource::File(f) => ("fichier".to_string(), Value::Str(f.clone())),
            SubSource::Embedded(n) => ("piste".to_string(), Value::Num(*n as f64)),
        }];
        o.push(("langue".into(), Value::Str(self.lang.clone())));
        if self.offset_ms != 0 {
            o.push(("decalage_ms".into(), Value::Num(self.offset_ms as f64)));
        }
        let s = &self.style;
        let mut st = Vec::new();
        if !s.font.is_empty() { st.push(("police".to_string(), Value::Str(s.font.clone()))); }
        st.push(("taille".into(), Value::Num(s.size as f64)));
        st.push(("couleur".into(), hex(s.color)));
        st.push(("contour_couleur".into(), hex(s.outline_color)));
        st.push(("contour".into(), Value::Num(s.outline as f64)));
        st.push(("position".into(), Value::Str(s.pos.name().into())));
        st.push(("marge".into(), Value::Num(s.margin as f64)));
        o.push(("style".into(), Value::Obj(st)));
        Value::Obj(o)
    }

    /// Problemes de reglage (liste vide si tout va bien).
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.lang.len() != 3 || !self.lang.is_ascii() { p.push("langue : 3 lettres attendues (ex. fra)".to_string()); }
        if let SubSource::File(f) = &self.source { if f.trim().is_empty() { p.push("fichier de sous-titres non choisi".to_string()); } }
        p
    }
}

fn parse_hex(s: &str) -> Result<[u8; 3], String> {
    let c = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| format!("couleur invalide : {s}"))?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Liste de pistes depuis la valeur JSON du champ « soustitres » d'une video (absent = aucune piste).
pub fn tracks_from_value(v: Option<&Value>) -> Result<Vec<SubTrackCfg>, String> {
    match v {
        None | Some(Value::Null) => Ok(vec![]),
        Some(a) => a.as_array().ok_or("« soustitres » : liste attendue")?.iter().map(SubTrackCfg::from_value).collect(),
    }
}

pub fn tracks_to_value(t: &[SubTrackCfg]) -> Option<Value> {
    if t.is_empty() { None } else { Some(Value::Arr(t.iter().map(SubTrackCfg::to_value).collect())) }
}
