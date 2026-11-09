//! Sous-titres texte : lecture de fichiers SRT et ASS/SSA, fusion des repliques simultanees, style d'affichage.
//! Les temps sont en millisecondes depuis le debut de la video.

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// Position demandee par le fichier (balise ASS `\an1`..`\an9`) ; `None` : celle de la piste.
    pub pos: Option<CuePos>,
}

/// Position d'une replique : alignement horizontal et vertical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CuePos {
    pub h: Option<HAlign>,
    pub v: Option<VPos>,
}

impl CuePos {
    /// Nom lisible (« haut gauche », « bas centre »...).
    pub fn label(self) -> String {
        match (self.v, self.h) {
            (Some(v), Some(h)) => format!("{} {}", v.name(), h.name()),
            (Some(v), None) => v.name().to_string(),
            (None, Some(h)) => h.name().to_string(),
            (None, None) => "piste".to_string(),
        }
    }
}

/// Position donnee par une balise `{\anN}` (N de 1 a 9, disposition du pave numerique) dans un texte brut ; la derniere l'emporte.
pub fn an_tag(raw: &str) -> Option<CuePos> {
    let mut found = None;
    let mut rest = raw;
    while let Some(i) = rest.find('{') {
        let Some(j) = rest[i..].find('}') else { break };
        let block = &rest[i + 1..i + j];
        let mut b = block;
        while let Some(k) = b.find("\\an") {
            if let Some(d) = b[k + 3..].chars().next().and_then(|c| c.to_digit(10)).filter(|d| (1..=9).contains(d)) {
                let v = match d { 7..=9 => VPos::Top, 4..=6 => VPos::Middle, _ => VPos::Bottom };
                let h = match d % 3 { 1 => HAlign::Left, 2 => HAlign::Center, _ => HAlign::Right };
                found = Some(CuePos { h: Some(h), v: Some(v) });
            }
            b = &b[k + 3..];
        }
        rest = &rest[i + j + 1..];
    }
    found
}

/// Retire les positions des repliques (la piste impose alors la sienne).
pub fn clear_positions(cues: &[Cue]) -> Vec<Cue> {
    cues.iter().map(|c| Cue { pos: None, ..c.clone() }).collect()
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

/// Alignement horizontal du bloc de texte (et des lignes dans le bloc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

impl HAlign {
    pub const ALL: [HAlign; 3] = [HAlign::Left, HAlign::Center, HAlign::Right];
    pub fn name(self) -> &'static str {
        match self { HAlign::Left => "gauche", HAlign::Center => "centre", HAlign::Right => "droite" }
    }
    pub fn from_name(s: &str) -> Option<HAlign> {
        match s { "gauche" => Some(HAlign::Left), "centre" => Some(HAlign::Center), "droite" => Some(HAlign::Right), _ => None }
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
    /// Alignement horizontal ; avec « gauche » ou « droite », le bloc est place a `margin_x` du bord.
    pub halign: HAlign,
    /// Marge laterale (px, pour 1080 lignes) utilisee par les alignements gauche et droite.
    pub margin_x: i32,
    /// Largeur maximale du texte, en % de la largeur de la video.
    pub max_width: i32,
    /// Coupe les lignes trop longues aux espaces (sinon la taille du texte est reduite).
    pub wrap: bool,
    /// Ombre portee : decalage en px (0 : aucune), couleur et opacite en %.
    pub shadow: i32,
    pub shadow_color: [u8; 3],
    pub shadow_opacity: i32,
    /// Boite derriere le texte : opacite en % (0 : aucune), couleur, marge interieure et rayon des coins (px).
    pub bg_opacity: i32,
    pub bg_color: [u8; 3],
    pub bg_pad: i32,
    pub bg_radius: i32,
}

impl Default for SubStyle {
    fn default() -> Self {
        SubStyle { font: String::new(), size: 56, color: [255, 255, 255], outline_color: [0, 0, 0], outline: 3, pos: VPos::Bottom, margin: 70,
            halign: HAlign::Center, margin_x: 100, max_width: 92, wrap: false,
            shadow: 0, shadow_color: [0, 0, 0], shadow_opacity: 70,
            bg_opacity: 0, bg_color: [0, 0, 0], bg_pad: 12, bg_radius: 8,
        }
    }
}

// ------------------------------------------------------------------ decodage du texte
/// Octets d'un fichier texte -> chaine : UTF-8 (avec ou sans BOM), UTF-16 avec BOM, sinon Windows-1252.
/// Pour une autre page de code, voir `charset::decode`.
pub fn decode_text(b: &[u8]) -> String {
    crate::charset::decode(b, crate::charset::Charset::Auto, crate::charset::Charset::Auto)
}

// ------------------------------------------------------------------ entites HTML
/// Remplace les entites HTML (`&amp;`, `&eacute;`, `&#233;`, `&#xE9;`...) ; `&lrm;`, `&rlm;`, `&zwj;` disparaissent, `&nbsp;` devient une espace.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let end = tail.char_indices().take(12).find(|(_, c)| *c == ';').map(|(k, _)| k);
        let decoded = end.and_then(|k| {
            let name = &tail[..k];
            let cp = if let Some(num) = name.strip_prefix('#') {
                match num.strip_prefix(['x', 'X']) { Some(h) => u32::from_str_radix(h, 16).ok(), None => num.parse::<u32>().ok() }
            } else {
                crate::entities::NAMED.binary_search_by(|(n, _)| n.cmp(&name)).ok().map(|x| crate::entities::NAMED[x].1)
            };
            cp.filter(|c| *c != 0).and_then(char::from_u32).map(|c| (c, k))
        });
        match decoded {
            Some((c, k)) => {
                match c {
                    '\u{200E}' | '\u{200F}' | '\u{200D}' | '\u{200C}' => {}
                    '\u{A0}' => out.push(' '),
                    _ => out.push(c),
                }
                rest = &tail[k + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------ mise en forme (italique, gras, souligne, couleur)
// Dans `Cue::text`, la mise en forme est portee par des marqueurs : U+0001 suivi de i/I (italique debut/fin), b/B, u/U,
// `c` + 6 chiffres hexadecimaux (couleur RRGGBB) ou `C` (couleur par defaut). Un caractere `<` du texte reste donc un `<`.
const MARK: char = '\u{1}';

/// Un morceau de texte de meme mise en forme.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<[u8; 3]>,
}

#[derive(Clone, Copy, PartialEq, Default)]
struct Fmt {
    i: bool,
    b: bool,
    u: bool,
    color: Option<[u8; 3]>,
}

fn emit(out: &mut String, cur: &mut Fmt, new: Fmt) {
    if cur.i != new.i { out.push(MARK); out.push(if new.i { 'i' } else { 'I' }); }
    if cur.b != new.b { out.push(MARK); out.push(if new.b { 'b' } else { 'B' }); }
    if cur.u != new.u { out.push(MARK); out.push(if new.u { 'u' } else { 'U' }); }
    if cur.color != new.color {
        out.push(MARK);
        match new.color {
            Some(c) => { out.push('c'); out.push_str(&format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])); }
            None => out.push('C'),
        }
    }
    *cur = new;
}

/// Lignes de morceaux d'un texte avec marqueurs ; la mise en forme se poursuit d'une ligne a l'autre.
pub fn parse_runs(text: &str) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut f = Fmt::default();
    let mut buf = String::new();
    let flush = |buf: &mut String, f: &Fmt, lines: &mut Vec<Vec<Run>>| {
        if !buf.is_empty() {
            lines.last_mut().unwrap().push(Run { text: std::mem::take(buf), bold: f.b, italic: f.i, underline: f.u, color: f.color });
        }
    };
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == MARK {
            flush(&mut buf, &f, &mut lines);
            match it.next() {
                Some('i') => f.i = true,
                Some('I') => f.i = false,
                Some('b') => f.b = true,
                Some('B') => f.b = false,
                Some('u') => f.u = true,
                Some('U') => f.u = false,
                Some('C') => f.color = None,
                Some('c') => {
                    let hex: String = (0..6).filter_map(|_| it.next()).collect();
                    f.color = u32::from_str_radix(&hex, 16).ok().map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8]);
                }
                _ => {}
            }
        } else if c == '\n' {
            flush(&mut buf, &f, &mut lines);
            lines.push(Vec::new());
        } else {
            buf.push(c);
        }
    }
    flush(&mut buf, &f, &mut lines);
    lines
}

/// Texte sans mise en forme (lignes separees par « \n »).
pub fn plain_text(text: &str) -> String {
    parse_runs(text).iter().map(|l| l.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
}

fn parse_color(v: &str) -> Option<[u8; 3]> {
    let v = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    let hex = v.strip_prefix('#').unwrap_or(v);
    if hex.chars().all(|c| c.is_ascii_hexdigit()) {
        if hex.len() == 6 {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some([(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        }
        if hex.len() == 3 {
            let d = |k: usize| u8::from_str_radix(&hex[k..k + 1], 16).ok().map(|x| x * 17);
            return Some([d(0)?, d(1)?, d(2)?]);
        }
    }
    Some(match v.to_ascii_lowercase().as_str() {
        "white" => [255, 255, 255], "black" => [0, 0, 0], "red" => [255, 0, 0], "lime" => [0, 255, 0], "green" => [0, 128, 0],
        "blue" => [0, 0, 255], "yellow" => [255, 255, 0], "cyan" | "aqua" => [0, 255, 255], "magenta" | "fuchsia" => [255, 0, 255],
        "gray" | "grey" => [128, 128, 128], "silver" => [192, 192, 192], "orange" => [255, 165, 0], "pink" => [255, 192, 203],
        "maroon" => [128, 0, 0], "navy" => [0, 0, 128], "olive" => [128, 128, 0], "purple" => [128, 0, 128], "teal" => [0, 128, 128],
        _ => return None,
    })
}

fn html_tag(tag: &str, w: &mut Fmt) {
    let t = tag.trim();
    let (close, body) = match t.strip_prefix('/') { Some(r) => (true, r.trim()), None => (false, t) };
    let name_end = body.find(|c: char| c.is_whitespace() || c == '.').unwrap_or(body.len());
    let name = body[..name_end].to_ascii_lowercase();
    let rest = &body[name_end..];
    match name.as_str() {
        "i" | "em" => w.i = !close,
        "b" | "strong" => w.b = !close,
        "u" => w.u = !close,
        "font" => {
            if close {
                w.color = None;
            } else if let Some(p) = rest.to_ascii_lowercase().find("color") {
                let v = rest[p + 5..].trim_start().strip_prefix('=').unwrap_or("").trim_start();
                let val: String = match v.chars().next() {
                    Some(q @ ('"' | '\'')) => v[1..].chars().take_while(|c| *c != q).collect(),
                    _ => v.chars().take_while(|c| !c.is_whitespace()).collect(),
                };
                if let Some(c) = parse_color(&val) { w.color = Some(c); }
            }
        }
        // WebVTT : <c.yellow>
        "c" => {
            if close {
                w.color = None;
            } else if let Some(c) = rest.trim_start_matches('.').split(['.', ' ']).next().and_then(parse_color) {
                w.color = Some(c);
            }
        }
        _ => {}
    }
}

/// Couleur ASS `&HBBGGRR&` (ou `$bbggrr` en MicroDVD) -> RGB.
fn bgr_color(s: &str) -> Option<[u8; 3]> {
    let h: String = s.chars().skip_while(|c| !c.is_ascii_hexdigit() || *c == 'H').take_while(|c| c.is_ascii_hexdigit()).collect();
    let h = if h.len() > 6 { &h[h.len() - 6..] } else { &h[..] };
    let n = u32::from_str_radix(h, 16).ok()?;
    Some([n as u8, (n >> 8) as u8, (n >> 16) as u8])
}

fn ass_block(blk: &str, w: &mut Fmt) {
    // MicroDVD : {y:i} {y:b} {c:$bbggrr}
    if let Some(r) = blk.strip_prefix("y:").or_else(|| blk.strip_prefix("Y:")) {
        for ch in r.chars() { match ch { 'i' => w.i = true, 'b' => w.b = true, 'u' => w.u = true, _ => {} } }
        return;
    }
    if let Some(r) = blk.strip_prefix("c:").or_else(|| blk.strip_prefix("C:")) {
        if let Some(c) = bgr_color(r.trim_start_matches('$')) { w.color = Some(c); }
        return;
    }
    let mut depth = 0i32;
    for tok in blk.split('\\').skip(1) {
        let (opens, closes) = (tok.matches('(').count() as i32, tok.matches(')').count() as i32);
        let inside = depth > 0;
        depth += opens - closes;
        if inside { continue; } // arguments d'une transformation animee : ignores
        let digit = |k: usize| tok.as_bytes().get(k).map_or(false, |b| b.is_ascii_digit());
        let on = |k: usize| tok[k..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().map_or(false, |n| n != 0);
        if tok.starts_with('r') {
            *w = Fmt::default();
        } else if tok.starts_with('i') && digit(1) {
            w.i = on(1);
        } else if tok.starts_with('b') && digit(1) {
            w.b = on(1);
        } else if tok.starts_with('u') && digit(1) {
            w.u = on(1);
        } else if tok == "c" || tok == "1c" {
            w.color = None;
        } else if tok.starts_with("c&") || tok.starts_with("1c&") {
            if let Some(c) = bgr_color(tok.trim_start_matches('1').trim_start_matches('c')) { w.color = Some(c); }
        }
    }
}

/// Texte brut d'un fichier de sous-titres -> texte avec marqueurs : balises HTML (`<i>`, `<b>`, `<u>`, `<font color>`, `<c.couleur>`) et
/// blocs ASS (`{\i1}`, `{\b1}`, `{\u1}`, `{\c&H..&}`, `{\r}`) sont convertis, les autres balises et blocs supprimes. La mise en forme est
/// refermee en fin de texte. Les entites HTML ne sont pas touchees (voir `decode_entities`).
pub fn convert_markup(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let (mut out, mut cur, mut want) = (String::new(), Fmt::default(), Fmt::default());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '<' || c == '{' {
            let close = if c == '<' { '>' } else { '}' };
            if let Some(j) = chars[i + 1..].iter().position(|&x| x == close) {
                let inner: String = chars[i + 1..i + 1 + j].iter().collect();
                if c == '<' { html_tag(&inner, &mut want); } else { ass_block(&inner, &mut want); }
                // une mise en forme qui se termine est refermee tout de suite (avant les espaces qui suivent) ; une qui commence attend le texte
                let mut now = cur;
                if !want.i { now.i = false; }
                if !want.b { now.b = false; }
                if !want.u { now.u = false; }
                if want.color.is_none() { now.color = None; }
                emit(&mut out, &mut cur, now);
                i += j + 2;
                continue;
            }
        }
        if !c.is_whitespace() { emit(&mut out, &mut cur, want); }
        out.push(c);
        i += 1;
    }
    emit(&mut out, &mut cur, Fmt::default());
    out
}

/// Balises converties puis entites decodees.
fn clean_markup(s: &str) -> String {
    decode_entities(&convert_markup(s))
}

/// Lignes nettoyees (espaces de bord retires) sans les lignes qui n'ont pas de texte visible.
fn tidy(t: &str) -> String {
    t.lines().map(str::trim).filter(|l| !plain_text(l).trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// Repliques sans mise en forme (italique, gras, souligne, couleur) : seul le texte reste.
pub fn strip_format(cues: &[Cue]) -> Vec<Cue> {
    cues.iter().map(|c| Cue { text: plain_text(&c.text), ..c.clone() }).collect()
}

/// Reglages WebVTT d'une replique (`line:`, `position:`, `align:`) -> position ; `size:` n'est pas pris en compte.
fn vtt_pos(tokens: &[&str]) -> Option<CuePos> {
    let (mut line, mut position, mut align): (Option<f64>, Option<f64>, Option<HAlign>) = (None, None, None);
    let mut seen = false;
    for t in tokens {
        if let Some(v) = t.strip_prefix("line:") {
            seen = true;
            let v = v.split(',').next().unwrap_or("");
            if let Some(p) = v.strip_suffix('%') {
                line = p.parse().ok();
            } else if let Ok(n) = v.parse::<f64>() {
                // numero de ligne : negatif = depuis le bas ; positif = rang parmi une quinzaine de lignes
                line = Some(if n < 0.0 { 100.0 } else { (n / 15.0 * 100.0).min(100.0) });
            }
        } else if let Some(v) = t.strip_prefix("position:") {
            seen = true;
            position = v.split(',').next().and_then(|x| x.strip_suffix('%')).and_then(|x| x.parse().ok());
        } else if let Some(v) = t.strip_prefix("align:") {
            seen = true;
            align = match v { "start" | "left" => Some(HAlign::Left), "end" | "right" => Some(HAlign::Right), "center" | "middle" => Some(HAlign::Center), _ => None };
        }
    }
    if !seen { return None; }
    let v = line.map(|p| if p < 33.4 { VPos::Top } else if p < 66.7 { VPos::Middle } else { VPos::Bottom });
    let h = match position {
        Some(p) if p < 33.4 => Some(HAlign::Left),
        Some(p) if p > 66.6 => Some(HAlign::Right),
        Some(_) => Some(HAlign::Center),
        None => align,
    };
    (v.is_some() || h.is_some()).then_some(CuePos { h, v })
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
    let mut cur: Option<(i64, i64, Vec<String>, Option<CuePos>)> = None;
    let flush = |cur: &mut Option<(i64, i64, Vec<String>, Option<CuePos>)>, cues: &mut Vec<Cue>| {
        if let Some((s, e, lines, pos)) = cur.take() {
            let t = tidy(&clean_markup(&lines.join("\n")));
            if !t.is_empty() && e > s {
                cues.push(Cue { start_ms: s, end_ms: e, text: t, pos });
            }
        }
    };
    for line in text.lines() {
        if let Some(i) = line.find("-->") {
            let (a, b) = (parse_srt_time(&line[..i]), line[i + 3..].split_whitespace().next().and_then(parse_srt_time));
            if let (Some(a), Some(b)) = (a, b) {
                flush(&mut cur, &mut cues);
                let settings: Vec<&str> = line[i + 3..].split_whitespace().skip(1).collect();
                cur = Some((a, b, Vec::new(), vtt_pos(&settings)));
                continue;
            }
        }
        if line.trim().is_empty() {
            flush(&mut cur, &mut cues);
        } else if let Some((_, _, lines, pos)) = cur.as_mut() {
            if let Some(p) = an_tag(line) { *pos = Some(p); }
            if !line.trim().is_empty() {
                lines.push(line.trim().to_string());
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

/// Position demandee par la colonne « Alignment » d'un style ASS (pave numerique 1..9) ou SSA (1-3 bas, 5-7 haut, 9-11 milieu).
/// Le bas centre, valeur par defaut, ne demande rien : la position de la piste s'applique.
fn style_pos(a: i64, legacy: bool) -> Option<CuePos> {
    let (v, h) = if legacy {
        let v = match a { 1..=3 => VPos::Bottom, 5..=7 => VPos::Top, 9..=11 => VPos::Middle, _ => return None };
        (v, (a - 1) % 4)
    } else {
        let v = match a { 1..=3 => VPos::Bottom, 4..=6 => VPos::Middle, 7..=9 => VPos::Top, _ => return None };
        (v, (a - 1) % 3)
    };
    let h = match h { 0 => HAlign::Left, 1 => HAlign::Center, _ => HAlign::Right };
    (v != VPos::Bottom || h != HAlign::Center).then_some(CuePos { h: Some(h), v: Some(v) })
}

pub fn parse_ass(text: &str) -> Vec<Cue> {
    use std::collections::HashMap;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    // 0 : autre section, 1 : styles, 2 : evenements
    let mut section = 0u8;
    let (mut cols, mut style_cols): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let (mut legacy, mut styles): (bool, HashMap<String, CuePos>) = (false, HashMap::new());
    let norm = |n: &str| n.trim().trim_start_matches('*').to_lowercase();
    let mut cues = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            let name = l.to_ascii_lowercase();
            section = match name.as_str() {
                "[events]" => 2,
                "[v4+ styles]" | "[v4++ styles]" => { legacy = false; 1 }
                "[v4 styles]" => { legacy = true; 1 }
                _ => 0,
            };
            continue;
        }
        if section == 0 {
            continue;
        }
        if section == 1 {
            if let Some(f) = l.strip_prefix("Format:") {
                style_cols = f.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
            } else if let Some(d) = l.strip_prefix("Style:") {
                let parts: Vec<&str> = d.splitn(style_cols.len().max(1), ',').collect();
                let col = |name: &str| style_cols.iter().position(|c| c == name);
                if let (Some(i_name), Some(i_al)) = (col("name"), col("alignment")) {
                    if let (Some(n), Some(a)) = (parts.get(i_name), parts.get(i_al).and_then(|x| x.trim().parse::<i64>().ok())) {
                        if let Some(p) = style_pos(a, legacy) { styles.insert(norm(n), p); }
                    }
                }
            }
            continue;
        }
        if let Some(f) = l.strip_prefix("Format:") {
            cols = f.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
        } else if let Some(d) = l.strip_prefix("Dialogue:") {
            let n = if cols.is_empty() { 10 } else { cols.len() };
            let parts: Vec<&str> = d.splitn(n, ',').collect();
            let col = |name: &str, dflt: usize| cols.iter().position(|c| c == name).unwrap_or(dflt);
            let (is, ie, it, iy) = (col("start", 1), col("end", 2), col("text", n - 1), col("style", 3));
            let (Some(a), Some(b), Some(t)) = (parts.get(is).and_then(|x| parse_ass_time(x)), parts.get(ie).and_then(|x| parse_ass_time(x)), parts.get(it)) else {
                continue;
            };
            // la balise \an de la replique l'emporte sur l'alignement de son style
            let pos = an_tag(t).or_else(|| parts.get(iy).and_then(|y| styles.get(&norm(y)).copied()));
            let t = tidy(&convert_markup(&t.replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " ")));
            if !t.is_empty() && b > a {
                cues.push(Cue { start_ms: a, end_ms: b, text: t, pos });
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
        let t = tidy(&clean_markup(&body.replace('|', "\n")));
        let (ms, me) = ((s as f64 * 1000.0 / fps).round() as i64, (e as f64 * 1000.0 / fps).round() as i64);
        if !t.is_empty() && me > ms { cues.push(Cue { start_ms: ms, end_ms: me, text: t, pos: None }); }
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
            if !t.is_empty() && e > s { cues.push(Cue { start_ms: s, end_ms: e, text: t, pos: None }); }
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
                let p = clean_markup(part);
                if !plain_text(&p).trim().is_empty() { lines.push(p.trim().to_string()); }
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
        let clean = tidy(&clean_markup(&t_body));
        items.push((t, clean));
    }
    let mut cues = Vec::new();
    for (k, (t, txt)) in items.iter().enumerate() {
        if txt.is_empty() { continue; }
        let end = items.get(k + 1).map(|n| n.0).unwrap_or(*t + 3000);
        if end > *t { cues.push(Cue { start_ms: *t, end_ms: end, text: txt.clone(), pos: None }); }
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
    load_cues_enc(path, fps, crate::charset::Charset::Auto, "")
}

/// Comme `load_cues_fps`, avec la page de code du fichier (`Auto` : UTF-8, sinon la page habituelle de la langue `lang`, code de 3 lettres).
pub fn load_cues_enc(path: &std::path::Path, fps: Option<f64>, charset: crate::charset::Charset, lang: &str) -> Result<Vec<Cue>, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut c = parse_file_text_fps(&ext, &crate::charset::decode(&b, charset, crate::charset::Charset::for_language(lang)), fps);
    if c.is_empty() && !ext.is_empty() && !NATIVE_EXT.contains(&ext.as_str()) {
        // TTML, LRC, STL, JACOsub... : ffmpeg les convertit en SRT
        let tools = crate::ffmpeg::Tools::locate().map_err(|e| trf!("{}: unrecognized format and ffmpeg not found ({})", path.display(), e))?;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dst = std::env::temp_dir().join(format!("bdauthor_sub_{}_{nanos}.srt", std::process::id()));
        let r = crate::ffmpeg::convert_subtitle_file_to_srt(&tools, path, &dst).map_err(|e| e.to_string());
        let t = std::fs::read(&dst);
        let _ = std::fs::remove_file(&dst);
        r?;
        c = parse_file_text("srt", &decode_text(&t.map_err(|e| e.to_string())?));
    }
    if c.is_empty() {
        return Err(trf!("{}: no readable cue", path.display()));
    }
    Ok(c)
}

// ------------------------------------------------------------------ fusion
/// Decale tous les temps ; les repliques entierement avant 0 disparaissent, celles qui chevauchent 0 sont tronquees.
pub fn shift(cues: &[Cue], offset_ms: i64) -> Vec<Cue> {
    cues.iter()
        .filter_map(|c| {
            let (s, e) = ((c.start_ms + offset_ms).max(0), c.end_ms + offset_ms);
            (e > s).then(|| Cue { start_ms: s, end_ms: e, text: c.text.clone(), pos: c.pos })
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
        let active: Vec<&&Cue> = cs.iter().filter(|c| c.start_ms <= a && c.end_ms >= b).collect();
        let t = active.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n");
        if t.is_empty() {
            continue;
        }
        // repliques simultanees : un seul bloc de texte, a la position de la premiere qui en demande une
        let pos = active.iter().find_map(|c| c.pos);
        match out.last_mut() {
            Some(l) if l.end_ms == a && l.text == t && l.pos == pos => l.end_ms = b,
            _ => out.push(Cue { start_ms: a, end_ms: b, text: t, pos }),
        }
    }
    out
}

/// Retire les indications entre crochets `[...]` ou parentheses `(...)` (sous-titres pour sourds et malentendants) ; une replique
/// qui n'a plus de texte disparait, une ligne vide est supprimee.
pub fn strip_notes(cues: &[Cue]) -> Vec<Cue> {
    fn clean_line(l: &str) -> String {
        let (mut out, mut depth) = (String::new(), 0usize);
        for c in l.chars() {
            match c {
                '[' | '(' => depth += 1,
                ']' | ')' if depth > 0 => depth -= 1,
                _ if depth == 0 => out.push(c),
                _ => {}
            }
        }
        // espaces doubles laisses par le retrait, et tiret de dialogue orphelin
        let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
        if plain_text(&t).trim().chars().all(|c| matches!(c, '-' | '\u{2013}' | '\u{2014}' | '\u{266a}' | '\u{266b}')) { String::new() } else { t }
    }
    // marqueurs de mise en forme d'une ligne supprimee : ils passent a la ligne suivante pour ne pas perdre l'italique, etc.
    fn markers_of(s: &str) -> String {
        let (mut out, mut it) = (String::new(), s.chars());
        while let Some(c) = it.next() {
            if c == MARK {
                out.push(c);
                if let Some(k) = it.next() {
                    out.push(k);
                    if k == 'c' { out.extend(it.by_ref().take(6)); }
                }
            }
        }
        out
    }
    cues.iter()
        .filter_map(|c| {
            let (mut lines, mut carry) = (Vec::new(), String::new());
            for l in c.text.lines() {
                let k = clean_line(l);
                if k.is_empty() {
                    carry.push_str(&markers_of(l));
                } else {
                    lines.push(format!("{carry}{k}"));
                    carry.clear();
                }
            }
            let t = lines.join("\n");
            (!t.is_empty()).then(|| Cue { start_ms: c.start_ms, end_ms: c.end_ms, text: t, pos: c.pos })
        })
        .collect()
}

// ------------------------------------------------------------------ controle de qualite
/// Seuils du controle de qualite (usage courant : 20 caracteres par seconde, 42 par ligne, 2 lignes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityLimits {
    pub max_cps: f64,
    pub max_line_chars: usize,
    pub max_lines: usize,
    pub min_ms: i64,
    pub max_ms: i64,
    pub min_gap_ms: i64,
}

impl Default for QualityLimits {
    fn default() -> Self {
        QualityLimits { max_cps: 20.0, max_line_chars: 42, max_lines: 2, min_ms: 800, max_ms: 7000, min_gap_ms: 80 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    TooFast,
    LongLine,
    TooManyLines,
    TooShort,
    TooLong,
    TightGap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QualityIssue {
    /// Rang de la replique dans la liste controlee.
    pub index: usize,
    pub kind: IssueKind,
    pub detail: String,
}

impl IssueKind {
    pub fn label(self) -> &'static str {
        match self {
            IssueKind::TooFast => tr("Reading too fast"),
            IssueKind::LongLine => tr("Line too long"),
            IssueKind::TooManyLines => tr("Too many lines"),
            IssueKind::TooShort => tr("Display too short"),
            IssueKind::TooLong => tr("Display too long"),
            IssueKind::TightGap => tr("Too close to the next one"),
        }
    }
}

/// Controle des repliques (deja fusionnees, decalage applique) : vitesse de lecture, longueur et nombre de lignes, durees, intervalle.
pub fn check_quality(cues: &[Cue], lim: &QualityLimits) -> Vec<QualityIssue> {
    let mut out = Vec::new();
    for (i, c) in cues.iter().enumerate() {
        let dur = c.end_ms - c.start_ms;
        let plain = plain_text(&c.text);
        let chars: usize = plain.lines().map(|l| l.trim().chars().count()).sum();
        let mut push = |kind, detail: String| out.push(QualityIssue { index: i, kind, detail });
        if dur > 0 {
            let cps = chars as f64 * 1000.0 / dur as f64;
            if cps > lim.max_cps { push(IssueKind::TooFast, trf!("{} characters/s (maximum {})", format!("{:.1}", cps), lim.max_cps)); }
        }
        if let Some(l) = plain.lines().map(|l| l.trim().chars().count()).max() {
            if l > lim.max_line_chars { push(IssueKind::LongLine, trf!("{} characters (maximum {})", l, lim.max_line_chars)); }
        }
        let n = plain.lines().count();
        if n > lim.max_lines { push(IssueKind::TooManyLines, trf!("{} lines (maximum {})", n, lim.max_lines)); }
        if dur < lim.min_ms { push(IssueKind::TooShort, trf!("{} ms (minimum {})", dur, lim.min_ms)); }
        if dur > lim.max_ms { push(IssueKind::TooLong, trf!("{} ms (maximum {})", dur, lim.max_ms)); }
        if let Some(nx) = cues.get(i + 1) {
            let gap = nx.start_ms - c.end_ms;
            if gap > 0 && gap < lim.min_gap_ms { push(IssueKind::TightGap, trf!("{} ms before the next one (minimum {})", gap, lim.min_gap_ms)); }
        }
    }
    out
}

// ------------------------------------------------------------------ reglages d'une piste (projet)
use crate::i18n::tr;
use crate::trf;
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
        let id = v.get("id").and_then(Value::as_i64).filter(|n| *n >= 1).ok_or(tr("languages: “id” (integer >= 1) expected"))? as u32;
        let code = v.get("code").and_then(Value::as_str).unwrap_or("und").to_string();
        let name = v.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| code.clone());
        Ok(Language { id, name, code })
    }
    pub fn to_value(&self) -> Value {
        Value::Obj(vec![("id".into(), Value::Num(self.id as f64)), ("name".into(), Value::Str(self.name.clone())), ("code".into(), Value::Str(self.code.clone()))])
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
        "fra" | "fre" => tr("French"), "eng" => tr("English"), "deu" | "ger" => tr("German"), "spa" => tr("Spanish"), "ita" => tr("Italian"),
        "por" => tr("Portuguese"), "nld" | "dut" => tr("Dutch"), "jpn" => tr("Japanese"), "zho" | "chi" => tr("Chinese"), "rus" => tr("Russian"),
        "ara" => tr("Arabic"), "kor" => tr("Korean"), "pol" => tr("Polish"), "swe" => tr("Swedish"), "dan" => tr("Danish"), "nor" => tr("Norwegian"),
        "fin" => tr("Finnish"), "tur" => tr("Turkish"), "heb" => tr("Hebrew"), "und" => tr("Unknown language"),
        c => return c.to_string(),
    }
    .to_string()
}

/// Liste des langues depuis la valeur JSON du champ « langues » (absent = liste vide).
pub fn languages_from_value(v: Option<&Value>) -> Result<Vec<Language>, String> {
    let out: Vec<Language> = match v {
        None | Some(Value::Null) => vec![],
        Some(a) => a.as_array().ok_or(tr("“languages”: list expected"))?.iter().map(Language::from_value).collect::<Result<_, _>>()?,
    };
    for (k, l) in out.iter().enumerate() {
        if out[..k].iter().any(|x| x.id == l.id) { return Err(trf!("languages: duplicate identifier {}", l.id)); }
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
    /// Retire les indications entre crochets ou parentheses ([musique], (rires)...) des repliques.
    pub strip_notes: bool,
    /// Respecte les positions donnees par le fichier (`\an1`..`\an9`) ; sinon toutes les repliques prennent la position de la piste.
    pub file_positions: bool,
    /// Garde l'italique, le gras, le souligne et les couleurs du fichier ; sinon tout le texte prend le style de la piste.
    pub keep_format: bool,
    /// Page de code du fichier texte (`Auto` : UTF-8, sinon celle de la langue de la piste).
    pub charset: crate::charset::Charset,
    pub style: SubStyle,
}

impl SubTrackCfg {
    pub fn from_file(path: &str, lang: &str) -> Self {
        SubTrackCfg { source: SubSource::File(path.into()), lang_id: 0, lang: lang.into(), offset_ms: 0, fps: None, strip_notes: false, file_positions: true, keep_format: true, charset: crate::charset::Charset::Auto, style: SubStyle::default() }
    }
    pub fn embedded(index: u32, lang: &str) -> Self {
        SubTrackCfg { source: SubSource::Embedded(index), lang_id: 0, lang: lang.into(), offset_ms: 0, fps: None, strip_notes: false, file_positions: true, keep_format: true, charset: crate::charset::Charset::Auto, style: SubStyle::default() }
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let source = match (v.get("file").and_then(Value::as_str), v.get("track").and_then(Value::as_i64)) {
            (Some(f), _) => SubSource::File(f.into()),
            (None, Some(n)) if n >= 0 => SubSource::Embedded(n as u32),
            _ => return Err(tr("subtitles: “file” or “track” expected").into()),
        };
        let mut st = SubStyle::default();
        if let Some(s) = v.get("style") {
            if let Some(x) = s.get("font").and_then(Value::as_str) { st.font = x.into(); }
            if let Some(x) = s.get("size").and_then(Value::as_i64) { st.size = x.clamp(8, 400) as i32; }
            if let Some(x) = s.get("color").and_then(Value::as_str) { st.color = parse_hex(x)?; }
            if let Some(x) = s.get("outline_color").and_then(Value::as_str) { st.outline_color = parse_hex(x)?; }
            if let Some(x) = s.get("outline").and_then(Value::as_i64) { st.outline = x.clamp(0, 30) as i32; }
            if let Some(x) = s.get("position").and_then(Value::as_str) { st.pos = VPos::from_name(x).ok_or_else(|| trf!("unknown subtitle position: {}", x))?; }
            if let Some(x) = s.get("margin").and_then(Value::as_i64) { st.margin = x.clamp(0, 540) as i32; }
            if let Some(x) = s.get("align").and_then(Value::as_str) { st.halign = HAlign::from_name(x).ok_or_else(|| trf!("unknown subtitle alignment: {}", x))?; }
            if let Some(x) = s.get("side_margin").and_then(Value::as_i64) { st.margin_x = x.clamp(0, 960) as i32; }
            if let Some(x) = s.get("max_width").and_then(Value::as_i64) { st.max_width = x.clamp(30, 100) as i32; }
            if let Some(x) = s.get("wrap").and_then(Value::as_bool) { st.wrap = x; }
            if let Some(x) = s.get("shadow").and_then(Value::as_i64) { st.shadow = x.clamp(0, 30) as i32; }
            if let Some(x) = s.get("shadow_color").and_then(Value::as_str) { st.shadow_color = parse_hex(x)?; }
            if let Some(x) = s.get("shadow_opacity").and_then(Value::as_i64) { st.shadow_opacity = x.clamp(0, 100) as i32; }
            if let Some(x) = s.get("background_opacity").and_then(Value::as_i64) { st.bg_opacity = x.clamp(0, 100) as i32; }
            if let Some(x) = s.get("background_color").and_then(Value::as_str) { st.bg_color = parse_hex(x)?; }
            if let Some(x) = s.get("background_margin").and_then(Value::as_i64) { st.bg_pad = x.clamp(0, 100) as i32; }
            if let Some(x) = s.get("background_radius").and_then(Value::as_i64) { st.bg_radius = x.clamp(0, 100) as i32; }
        }
        Ok(SubTrackCfg {
            source,
            lang_id: v.get("language_id").and_then(Value::as_i64).filter(|n| *n >= 1).map_or(0, |n| n as u32),
            lang: v.get("language").and_then(Value::as_str).unwrap_or("und").into(),
            offset_ms: v.get("offset_ms").and_then(Value::as_f64).map_or(0, |x| x.round() as i64),
            strip_notes: v.get("strip_notes").and_then(Value::as_bool).unwrap_or(false),
            file_positions: v.get("file_positions").and_then(Value::as_bool).unwrap_or(true),
            keep_format: v.get("keep_formatting").and_then(Value::as_bool).unwrap_or(true),
            charset: match v.get("encoding").and_then(Value::as_str) {
                None => crate::charset::Charset::Auto,
                Some(x) => crate::charset::Charset::from_name(x).ok_or_else(|| trf!("unknown subtitle encoding: {}", x))?,
            },
            fps: v.get("fps").and_then(Value::as_f64).filter(|f| f.is_finite() && *f >= 1.0 && *f <= 240.0),
            style: st,
        })
    }

    pub fn to_value(&self) -> Value {
        let hex = |c: [u8; 3]| Value::Str(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
        let mut o = vec![match &self.source {
            SubSource::File(f) => ("file".to_string(), Value::Str(f.clone())),
            SubSource::Embedded(n) => ("track".to_string(), Value::Num(*n as f64)),
        }];
        o.push(("language".into(), Value::Str(self.lang.clone())));
        if self.lang_id != 0 {
            o.push(("language_id".into(), Value::Num(self.lang_id as f64)));
        }
        if self.offset_ms != 0 {
            o.push(("offset_ms".into(), Value::Num(self.offset_ms as f64)));
        }
        if let Some(f) = self.fps { o.push(("fps".into(), Value::Num(f))); }
        if self.strip_notes { o.push(("strip_notes".into(), Value::Bool(true))); }
        if !self.file_positions { o.push(("file_positions".into(), Value::Bool(false))); }
        if !self.keep_format { o.push(("keep_formatting".into(), Value::Bool(false))); }
        if self.charset != crate::charset::Charset::Auto { o.push(("encoding".into(), Value::Str(self.charset.name().into()))); }
        let s = &self.style;
        let mut st = Vec::new();
        if !s.font.is_empty() { st.push(("font".to_string(), Value::Str(s.font.clone()))); }
        st.push(("size".into(), Value::Num(s.size as f64)));
        st.push(("color".into(), hex(s.color)));
        st.push(("outline_color".into(), hex(s.outline_color)));
        st.push(("outline".into(), Value::Num(s.outline as f64)));
        st.push(("position".into(), Value::Str(s.pos.name().into())));
        st.push(("margin".into(), Value::Num(s.margin as f64)));
        let d = SubStyle::default();
        if s.halign != d.halign { st.push(("align".into(), Value::Str(s.halign.name().into()))); }
        if s.margin_x != d.margin_x { st.push(("side_margin".into(), Value::Num(s.margin_x as f64))); }
        if s.max_width != d.max_width { st.push(("max_width".into(), Value::Num(s.max_width as f64))); }
        if s.wrap { st.push(("wrap".into(), Value::Bool(true))); }
        if s.shadow != 0 {
            st.push(("shadow".into(), Value::Num(s.shadow as f64)));
            st.push(("shadow_color".into(), hex(s.shadow_color)));
            st.push(("shadow_opacity".into(), Value::Num(s.shadow_opacity as f64)));
        }
        if s.bg_opacity != 0 {
            st.push(("background_opacity".into(), Value::Num(s.bg_opacity as f64)));
            st.push(("background_color".into(), hex(s.bg_color)));
            st.push(("background_margin".into(), Value::Num(s.bg_pad as f64)));
            st.push(("background_radius".into(), Value::Num(s.bg_radius as f64)));
        }
        o.push(("style".into(), Value::Obj(st)));
        Value::Obj(o)
    }

    /// Problemes de reglage (liste vide si tout va bien).
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.lang.len() != 3 || !self.lang.is_ascii() { p.push(tr("language: 3 letters expected (e.g. fra)").to_string()); }
        if let SubSource::File(f) = &self.source { if f.trim().is_empty() { p.push(tr("subtitle file not chosen").to_string()); } }
        p
    }
}

fn parse_hex(s: &str) -> Result<[u8; 3], String> {
    let c = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| trf!("invalid color: {}", s))?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Liste de pistes depuis la valeur JSON du champ « soustitres » d'une video (absent = aucune piste).
pub fn tracks_from_value(v: Option<&Value>) -> Result<Vec<SubTrackCfg>, String> {
    match v {
        None | Some(Value::Null) => Ok(vec![]),
        Some(a) => a.as_array().ok_or(tr("“subtitles”: list expected"))?.iter().map(SubTrackCfg::from_value).collect(),
    }
}

pub fn tracks_to_value(t: &[SubTrackCfg]) -> Option<Value> {
    if t.is_empty() { None } else { Some(Value::Arr(t.iter().map(SubTrackCfg::to_value).collect())) }
}
