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
    s.replace("&lrm;", "").replace("&rlm;", "").replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
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
    // « mm:ss.mmm » (WebVTT) ou « hh:mm:ss,mmm »
    let first = it.next()??;
    let second = it.next()??;
    let (h, m, sec) = match it.next() { Some(t) => (first, second, t?), None => (0, first, second) };
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

// ------------------------------------------------------------------ autres formats texte
/// MicroDVD (`{debut}{fin}texte`, `|` = saut de ligne) : temps en images ; la cadence vient d'une premiere ligne `{1}{1}23.976`, sinon `fps`.
pub fn parse_microdvd(text: &str, fps: f64) -> Vec<Cue> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut fps = if fps > 1.0 { fps } else { 23.976 };
    let mut cues = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let l = line.trim();
        let Some(r) = l.strip_prefix('{') else { continue };
        let Some((start, r)) = r.split_once('}') else { continue };
        let Some(r) = r.strip_prefix('{') else { continue };
        let Some((end, body)) = r.split_once('}') else { continue };
        let (Ok(s), Ok(e)) = (start.trim().parse::<i64>(), end.trim().parse::<i64>()) else { continue };
        if n < 3 && s == 1 && e == 1 {
            if let Ok(f) = body.trim().parse::<f64>() { if f > 1.0 { fps = f; } }
            continue;
        }
        let t = clean_entities(&strip_tags(&body.replace('|', "\n")));
        let t = t.lines().map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n");
        let (ms, me) = ((s as f64 * 1000.0 / fps).round() as i64, (e as f64 * 1000.0 / fps).round() as i64);
        if !t.is_empty() && me > ms { cues.push(Cue { start_ms: ms, end_ms: me, text: t }); }
    }
    cues
}

/// SubViewer 2 et SBV : une ligne `debut,fin` suivie du texte (`[br]` = saut de ligne), separees par des lignes vides.
pub fn parse_subviewer(text: &str) -> Vec<Cue> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    let mut cur: Option<(i64, i64, Vec<String>)> = None;
    let flush = |cur: &mut Option<(i64, i64, Vec<String>)>, cues: &mut Vec<Cue>| {
        if let Some((s, e, lines)) = cur.take() {
            let t = lines.join("\n").trim().to_string();
            if !t.is_empty() && e > s { cues.push(Cue { start_ms: s, end_ms: e, text: t }); }
        }
    };
    for line in text.lines() {
        let l = line.trim();
        if let Some((a, b)) = l.split_once(',') {
            if let (Some(s), Some(e)) = (parse_srt_time(a), parse_srt_time(b)) {
                flush(&mut cur, &mut cues);
                cur = Some((s, e, Vec::new()));
                continue;
            }
        }
        if l.is_empty() {
            flush(&mut cur, &mut cues);
        } else if let Some((_, _, lines)) = cur.as_mut() {
            for part in l.replace("[br]", "\n").lines() {
                let p = clean_entities(&strip_tags(part));
                if !p.trim().is_empty() { lines.push(p.trim().to_string()); }
            }
        }
    }
    flush(&mut cur, &mut cues);
    cues
}

/// SAMI (`.smi`) : blocs `<SYNC Start=ms>` ; une replique dure jusqu'au bloc suivant, un bloc vide (`&nbsp;`) l'efface.
/// Si le fichier porte plusieurs langues (classes de `<P>`), seule la premiere classe rencontree est lue.
pub fn parse_sami(text: &str) -> Vec<Cue> {
    let low = text.to_ascii_lowercase();
    let mut starts: Vec<usize> = Vec::new();
    let mut from = 0;
    while let Some(i) = low[from..].find("<sync") { starts.push(from + i); from += i + 5; }
    let mut items: Vec<(i64, String)> = Vec::new();
    let mut class: Option<String> = None;
    for (k, &st) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(text.len());
        let block = &text[st..end];
        let bl = &low[st..end];
        let Some(gt) = block.find('>') else { continue };
        let head = &bl[..gt];
        let Some(p) = head.find("start") else { continue };
        let digits: String = head[p + 5..].chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        let Ok(t) = digits.parse::<i64>() else { continue };
        let body = &block[gt + 1..];
        let bodyl = &bl[gt + 1..];
        if let Some(pp) = bodyl.find("<p") {
            let tag_end = bodyl[pp..].find('>').map(|x| pp + x).unwrap_or(pp);
            let tag = &bodyl[pp..tag_end];
            let cl = tag.find("class").map(|c| tag[c + 5..].trim_start_matches(|ch: char| ch == '=' || ch == ' ' || ch == '"' || ch == '\'').chars().take_while(|ch| ch.is_alphanumeric() || *ch == '_').collect::<String>());
            if let Some(cl) = cl {
                match &class { None => class = Some(cl), Some(c0) if *c0 != cl => continue, _ => {} }
            }
        }
        let t_body = body.replace("<br>", "\n").replace("<BR>", "\n").replace("<Br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
        let clean = clean_entities(&strip_tags(&t_body));
        let clean = clean.lines().map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n");
        items.push((t, clean));
    }
    let mut cues = Vec::new();
    for (k, (t, txt)) in items.iter().enumerate() {
        if txt.is_empty() { continue; }
        let end = items.get(k + 1).map(|n| n.0).unwrap_or(*t + 3000);
        if end > *t { cues.push(Cue { start_ms: *t, end_ms: end, text: txt.clone() }); }
    }
    cues
}

/// Format reconnu d'apres le contenu (l'extension est trop souvent trompeuse : `.sub`, `.txt`, `.xml`).
fn sniff(text: &str) -> &'static str {
    let low: String = text.chars().take(4000).collect::<String>().to_ascii_lowercase();
    if low.contains("<sami") { return "sami"; }
    if low.contains("[events]") { return "ass"; }
    if low.lines().map(str::trim).filter(|l| !l.is_empty()).take(3).any(|l| l.starts_with('{') && l.matches('{').count() >= 2 && l.matches('}').count() >= 2 && l[1..].chars().next().map_or(false, |c| c.is_ascii_digit())) { return "microdvd"; }
    if low.contains("-->") { return "srt"; }
    if text.lines().take(60).any(|l| l.split_once(',').map_or(false, |(a, b)| parse_srt_time(a).is_some() && parse_srt_time(b).is_some() && a.contains(':'))) { return "subviewer"; }
    "srt"
}

/// Choisit le format (`.ass` / `.ssa` d'apres l'extension, les autres d'apres le contenu : SRT, WebVTT, MicroDVD, SubViewer, SBV, SAMI), puis ordonne par debut.
pub fn parse_file_text(ext: &str, text: &str) -> Vec<Cue> {
    parse_file_text_fps(ext, text, None)
}

/// Comme `parse_file_text`, avec la cadence (images/s) a utiliser pour les formats dont les temps sont en images (MicroDVD).
pub fn parse_file_text_fps(ext: &str, text: &str, fps: Option<f64>) -> Vec<Cue> {
    let kind = if matches!(ext.to_ascii_lowercase().as_str(), "ass" | "ssa") { "ass" } else { sniff(text) };
    let mut c = match kind {
        "ass" => parse_ass(text),
        "sami" => parse_sami(text),
        "microdvd" => parse_microdvd(text, fps.unwrap_or(23.976)),
        "subviewer" => parse_subviewer(text),
        _ => parse_srt(text),
    };
    c.sort_by_key(|x| (x.start_ms, x.end_ms));
    c
}

/// Cadence mesuree d'une video (images/s) si c'est une cadence courante (23,976, 24, 25, 29,97, 30, 50, 59,94, 60) ; sinon `None`
/// (video a cadence variable ou mal lue) : on ne s'en sert pas pour convertir des numeros d'images en temps.
pub fn plausible_fps(f: f64) -> Option<f64> {
    [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 59.94, 60.0].iter().any(|r| (f - r).abs() < 0.02).then_some(f)
}

/// Extensions lues directement (les autres formats texte passent par ffmpeg s'il est installe).
const NATIVE_EXT: [&str; 9] = ["srt", "ass", "ssa", "vtt", "smi", "sami", "sub", "txt", "sbv"];

pub fn load_cues(path: &std::path::Path) -> Result<Vec<Cue>, String> {
    load_cues_fps(path, None)
}

/// Comme `load_cues`, avec la cadence des formats a temps en images (voir `SubTrackCfg::fps`).
pub fn load_cues_fps(path: &std::path::Path, fps: Option<f64>) -> Result<Vec<Cue>, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut c = parse_file_text_fps(&ext, &decode_text(&b), fps);
    if c.is_empty() && !ext.is_empty() && !NATIVE_EXT.contains(&ext.as_str()) {
        // TTML, LRC, STL, JACOsub... : ffmpeg les convertit en SRT
        let tools = crate::ffmpeg::Tools::locate().map_err(|e| format!("{}: format non reconnu et ffmpeg introuvable ({e})", path.display()))?;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dst = std::env::temp_dir().join(format!("bdauthor_sub_{}_{nanos}.srt", std::process::id()));
        let r = crate::ffmpeg::convert_subtitle_file_to_srt(&tools, path, &dst).map_err(|e| e.to_string());
        let t = std::fs::read(&dst);
        let _ = std::fs::remove_file(&dst);
        r?;
        c = parse_file_text("srt", &decode_text(&t.map_err(|e| e.to_string())?));
    }
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

/// Langue de sous-titres du projet : identifiant interne stable, nom affiche et code de 3 lettres.
/// Son rang dans la liste du projet est le numero de piste qu'elle a sur le disque, dans toutes les videos.
#[derive(Debug, Clone, PartialEq)]
pub struct Language {
    pub id: u32,
    pub name: String,
    pub code: String,
}

impl Language {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let id = v.get("id").and_then(Value::as_i64).filter(|n| *n >= 1).ok_or("langues : « id » (entier >= 1) attendu")? as u32;
        let code = v.get("code").and_then(Value::as_str).unwrap_or("und").to_string();
        let name = v.get("nom").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| code.clone());
        Ok(Language { id, name, code })
    }
    pub fn to_value(&self) -> Value {
        Value::Obj(vec![("id".into(), Value::Num(self.id as f64)), ("nom".into(), Value::Str(self.name.clone())), ("code".into(), Value::Str(self.code.clone()))])
    }
    /// Code sur 3 octets (`und` si invalide).
    pub fn code_bytes(&self) -> [u8; 3] {
        self.code.as_bytes().try_into().unwrap_or(*b"und")
    }
}

/// Identifiant libre pour une nouvelle langue de la liste.
pub fn next_language_id(langs: &[Language]) -> u32 {
    langs.iter().map(|l| l.id).max().unwrap_or(0) + 1
}

/// Nom usuel d'une langue d'apres son code ISO 639-2 (code lui-meme sinon).
pub fn default_language_name(code: &str) -> String {
    match code {
        "fra" | "fre" => "Français", "eng" => "Anglais", "deu" | "ger" => "Allemand", "spa" => "Espagnol", "ita" => "Italien",
        "por" => "Portugais", "nld" | "dut" => "Néerlandais", "jpn" => "Japonais", "zho" | "chi" => "Chinois", "rus" => "Russe",
        "ara" => "Arabe", "kor" => "Coréen", "pol" => "Polonais", "swe" => "Suédois", "dan" => "Danois", "nor" => "Norvégien",
        "fin" => "Finnois", "tur" => "Turc", "heb" => "Hébreu", "und" => "Langue inconnue",
        c => return c.to_string(),
    }
    .to_string()
}

/// Liste des langues depuis la valeur JSON du champ « langues » (absent = liste vide).
pub fn languages_from_value(v: Option<&Value>) -> Result<Vec<Language>, String> {
    let out: Vec<Language> = match v {
        None | Some(Value::Null) => vec![],
        Some(a) => a.as_array().ok_or("« langues » : liste attendue")?.iter().map(Language::from_value).collect::<Result<_, _>>()?,
    };
    for (k, l) in out.iter().enumerate() {
        if out[..k].iter().any(|x| x.id == l.id) { return Err(format!("langues : identifiant {} en double", l.id)); }
    }
    Ok(out)
}

pub fn languages_to_value(l: &[Language]) -> Option<Value> {
    if l.is_empty() { None } else { Some(Value::Arr(l.iter().map(Language::to_value).collect())) }
}

/// Une piste de sous-titres d'une video.
#[derive(Debug, Clone, PartialEq)]
pub struct SubTrackCfg {
    pub source: SubSource,
    /// Langue de la liste du projet (`Language::id`) ; 0 : non choisie.
    pub lang_id: u32,
    /// Code de langue de 3 lettres (ex. fra). Avec une langue du projet, c'est le code de cette langue.
    pub lang: String,
    /// Decalage a appliquer aux temps (ms, peut etre negatif).
    pub offset_ms: i64,
    /// Cadence (images/s) des fichiers dont les temps sont en images (MicroDVD) quand le fichier ne l'indique pas ; `None` : 23,976.
    pub fps: Option<f64>,
    pub style: SubStyle,
}

impl SubTrackCfg {
    pub fn from_file(path: &str, lang: &str) -> Self {
        SubTrackCfg { source: SubSource::File(path.into()), lang_id: 0, lang: lang.into(), offset_ms: 0, fps: None, style: SubStyle::default() }
    }
    pub fn embedded(index: u32, lang: &str) -> Self {
        SubTrackCfg { source: SubSource::Embedded(index), lang_id: 0, lang: lang.into(), offset_ms: 0, fps: None, style: SubStyle::default() }
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
            lang_id: v.get("langue_id").and_then(Value::as_i64).filter(|n| *n >= 1).map_or(0, |n| n as u32),
            lang: v.get("langue").and_then(Value::as_str).unwrap_or("und").into(),
            offset_ms: v.get("decalage_ms").and_then(Value::as_f64).map_or(0, |x| x.round() as i64),
            fps: v.get("cadence_ips").and_then(Value::as_f64).filter(|f| f.is_finite() && *f >= 1.0 && *f <= 240.0),
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
        if self.lang_id != 0 {
            o.push(("langue_id".into(), Value::Num(self.lang_id as f64)));
        }
        if self.offset_ms != 0 {
            o.push(("decalage_ms".into(), Value::Num(self.offset_ms as f64)));
        }
        if let Some(f) = self.fps { o.push(("cadence_ips".into(), Value::Num(f))); }
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
