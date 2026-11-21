//! Mise en forme d'une partie d'un texte de jaquette : portions de texte (en caracteres) en gras, en italique ou d'une autre
//! couleur que le reste. Chaque reglage d'une portion remplace celui du texte entier ; `None` garde celui du texte.
use crate::json::Value;
use std::collections::BTreeMap;

/// Reglages d'une portion ; `None` : comme le texte entier.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpanStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub color: Option<[u8; 3]>,
    /// taille en points ; dans un `cover::CoverDecor::Text`, facteur de la taille du texte (voir `cover::item_decor`)
    pub size: Option<f64>,
}

impl SpanStyle {
    pub fn is_plain(&self) -> bool { *self == SpanStyle::default() }
    /// Gras, italique et couleur effectifs, d'apres ceux du texte entier.
    pub fn resolve(&self, bold: bool, italic: bool, color: [u8; 3]) -> (bool, bool, [u8; 3]) {
        (self.bold.unwrap_or(bold), self.italic.unwrap_or(italic), self.color.unwrap_or(color))
    }
    /// Facteur de taille d'un caractere dessine (1 : taille du texte).
    pub fn factor(&self) -> f32 { self.size.map_or(1.0, |s| s as f32) }
}

/// Portion `start..end` (rangs de caracteres, `end` exclu) mise en forme par `style`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
    pub style: SpanStyle,
}

/// Reglage de chacun des `n` caracteres.
pub fn per_char(n: usize, spans: &[TextSpan]) -> Vec<SpanStyle> {
    let mut v = vec![SpanStyle::default(); n];
    for s in spans {
        for c in v.iter_mut().take(s.end.min(n)).skip(s.start) { *c = s.style; }
    }
    v
}

/// Portions d'apres le reglage de chaque caractere : caracteres voisins de meme reglage regroupes, texte simple omis.
pub fn from_per_char(styles: &[SpanStyle]) -> Vec<TextSpan> {
    let mut out: Vec<TextSpan> = Vec::new();
    for (i, st) in styles.iter().enumerate() {
        if st.is_plain() { continue; }
        match out.last_mut() {
            Some(l) if l.end == i && l.style == *st => l.end = i + 1,
            _ => out.push(TextSpan { start: i, end: i + 1, style: *st }),
        }
    }
    out
}

/// Portions bornees a un texte de `n` caracteres, triees, sans chevauchement.
pub fn normalize(spans: &[TextSpan], n: usize) -> Vec<TextSpan> { from_per_char(&per_char(n, spans)) }

/// Modifie par `f` le reglage des caracteres `range` (texte de `n` caracteres).
pub fn apply(spans: &[TextSpan], n: usize, range: std::ops::Range<usize>, f: impl Fn(&mut SpanStyle)) -> Vec<TextSpan> {
    let mut v = per_char(n, spans);
    for c in v.iter_mut().take(range.end.min(n)).skip(range.start) { f(c); }
    from_per_char(&v)
}

/// Reglage commun a tous les caracteres de `range` (`None` pour un reglage qui varie, ou une plage vide).
pub fn common(spans: &[TextSpan], n: usize, range: std::ops::Range<usize>) -> Option<SpanStyle> {
    let v = per_char(n, spans);
    let mut it = v.get(range.start.min(n)..range.end.min(n))?.iter();
    let first = *it.next()?;
    it.all(|s| *s == first).then_some(first)
}

/// Portions apres la modification du texte `old` en `new` : la partie changee est reperee par le debut et la fin communs aux deux
/// textes ; ce qui est tape prend le reglage du caractere qui le precede (ou, au debut du texte, de celui qui le suit).
pub fn after_edit(spans: &[TextSpan], old: &str, new: &str) -> Vec<TextSpan> {
    if spans.is_empty() || old == new { return spans.to_vec(); }
    let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let st = per_char(a.len(), spans);
    let inserted = b.len() - pre - suf;
    let fill = if pre > 0 { st[pre - 1] } else { st.get(a.len() - suf).copied().unwrap_or_default() };
    let mut out = st[..pre].to_vec();
    out.extend(std::iter::repeat_n(fill, inserted));
    out.extend_from_slice(&st[a.len() - suf..]);
    from_per_char(&out)
}

/// Morceaux consecutifs du texte et leur reglage (un seul morceau sans mise en forme).
pub fn runs(text: &str, spans: &[TextSpan]) -> Vec<(String, SpanStyle)> {
    let mut out: Vec<(String, SpanStyle)> = Vec::new();
    let st = per_char(text.chars().count(), spans);
    for (c, s) in text.chars().zip(st) {
        match out.last_mut() {
            Some((t, l)) if *l == s => t.push(c),
            _ => out.push((c.to_string(), s)),
        }
    }
    out
}

/// Texte affiche d'un texte saisi : champs `{nom}` remplaces par leur valeur (un champ inconnu reste tel quel), portions
/// reportees sur le texte affiche (la valeur d'un champ prend le reglage de son `{`), et correspondance des positions.
#[derive(Debug, Clone, PartialEq)]
pub struct Substituted {
    pub text: String,
    pub spans: Vec<TextSpan>,
    /// position (rang de caractere) dans le texte affiche de chaque position 0..=n du texte saisi ; une position a l'interieur
    /// d'un champ donne le debut de sa valeur
    pub disp: Vec<usize>,
    /// champs remplaces : rangs de caracteres dans le texte saisi (accolades comprises)
    pub fields: Vec<std::ops::Range<usize>>,
}

/// Remplace les champs `{nom}` connus de `vars`, de gauche a droite (une valeur n'est pas relue), et suit les positions.
pub fn substitute_map(text: &str, spans: &[TextSpan], vars: &BTreeMap<String, String>) -> Substituted {
    let raw: Vec<char> = text.chars().collect();
    let st = per_char(raw.len(), spans);
    let (mut out, mut ost, mut disp, mut fields) = (String::new(), Vec::new(), Vec::with_capacity(raw.len() + 1), Vec::new());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == '{' {
            if let Some(j) = raw[i + 1..].iter().position(|&c| c == '}').map(|p| i + 1 + p) {
                let key: String = raw[i + 1..j].iter().collect();
                if let Some(v) = vars.get(&key) {
                    let d0 = ost.len();
                    for _ in i..=j { disp.push(d0); }
                    out.push_str(v);
                    ost.extend(std::iter::repeat_n(st[i], v.chars().count()));
                    fields.push(i..j + 1);
                    i = j + 1;
                    continue;
                }
            }
        }
        disp.push(ost.len());
        out.push(raw[i]);
        ost.push(st[i]);
        i += 1;
    }
    disp.push(ost.len());
    Substituted { text: out, spans: from_per_char(&ost), disp, fields }
}

/// Remplace les champs `{nom}` (voir `substitute_map`) et rend le texte obtenu avec ses portions.
pub fn substitute(text: &str, spans: &[TextSpan], vars: &BTreeMap<String, String>) -> (String, Vec<TextSpan>) {
    let s = substitute_map(text, spans, vars);
    (s.text, s.spans)
}

// ------------------------------------------------------------------ JSON
fn hex(c: [u8; 3]) -> Value { Value::Str(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])) }

pub fn to_value(spans: &[TextSpan]) -> Value {
    Value::Arr(spans.iter().map(|s| {
        let mut o = vec![("start".to_string(), Value::Num(s.start as f64)), ("end".to_string(), Value::Num(s.end as f64))];
        if let Some(b) = s.style.bold { o.push(("bold".into(), Value::Bool(b))); }
        if let Some(i) = s.style.italic { o.push(("italic".into(), Value::Bool(i))); }
        if let Some(c) = s.style.color { o.push(("color".into(), hex(c))); }
        if let Some(z) = s.style.size { o.push(("size_pt".into(), Value::Num(z))); }
        Value::Obj(o)
    }).collect())
}

/// Portions lues dans `v` (entrees invalides ignorees), bornees au texte de `n` caracteres.
pub fn from_value(v: Option<&Value>, n: usize) -> Vec<TextSpan> {
    let Some(a) = v.and_then(Value::as_array) else { return vec![] };
    let spans: Vec<TextSpan> = a.iter().filter_map(|e| {
        let num = |k: &str| e.get(k).and_then(Value::as_f64).filter(|x| x.is_finite() && *x >= 0.0).map(|x| x as usize);
        let color = e.get("color").and_then(Value::as_str).and_then(|s| {
            let s = s.trim_start_matches('#');
            u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6).map(|n| [(n >> 16) as u8, (n >> 8) as u8, n as u8])
        });
        Some(TextSpan { start: num("start")?, end: num("end")?, style: SpanStyle { bold: e.get("bold").and_then(Value::as_bool), italic: e.get("italic").and_then(Value::as_bool), color,
            size: e.get("size_pt").and_then(Value::as_f64).filter(|z| z.is_finite()).map(|z| z.clamp(1.0, 600.0)) } })
    }).collect();
    normalize(&spans, n)
}
