//! Codes-barres lineaires : Code 128, EAN-13, EAN-8 et UPC-A. Ce module ne fait que l'encodage en modules (barres noires et
//! blancs de largeur 1) ; le dessin est fait par la mise en page de la jaquette (`cover`), qui pose un rectangle par barre.
use crate::i18n::tr;
use crate::trf;

/// Type de code-barres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarcodeKind { Code128, Ean13, Ean8, UpcA }

impl BarcodeKind {
    pub const ALL: [BarcodeKind; 4] = [BarcodeKind::Code128, BarcodeKind::Ean13, BarcodeKind::Ean8, BarcodeKind::UpcA];
    /// Nom dans le fichier de projet.
    pub fn name(self) -> &'static str {
        match self { BarcodeKind::Code128 => "code128", BarcodeKind::Ean13 => "ean13", BarcodeKind::Ean8 => "ean8", BarcodeKind::UpcA => "upca" }
    }
    pub fn from_name(s: &str) -> Option<BarcodeKind> { BarcodeKind::ALL.into_iter().find(|k| k.name() == s) }
    pub fn label(self) -> &'static str {
        match self { BarcodeKind::Code128 => "Code 128", BarcodeKind::Ean13 => "EAN-13", BarcodeKind::Ean8 => "EAN-8", BarcodeKind::UpcA => "UPC-A" }
    }
    /// Valeur d'exemple valide.
    pub fn sample(self) -> &'static str {
        match self { BarcodeKind::Code128 => "BD-2026", BarcodeKind::Ean13 => "5901234123457", BarcodeKind::Ean8 => "96385074", BarcodeKind::UpcA => "036000291452" }
    }
}

/// Code-barres encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Barcode {
    /// modules de gauche a droite : `true` = barre (zones de silence non comprises)
    pub modules: Vec<bool>,
    /// texte lisible sous les barres (chiffre de controle compris, groupes separes par des espaces)
    pub text: String,
}

/// Zone de silence a laisser de chaque cote des barres, en modules.
pub const QUIET_MODULES: usize = 10;

/// Largeurs (barre, espace, barre, espace, barre, espace) des 106 symboles du Code 128 puis de l'arret (7 elements).
pub const C128: [&str; 107] = [
    "212222", "222122", "222221", "121223", "121322", "131222", "122213", "122312", "132212", "221213",
    "221312", "231212", "112232", "122132", "122231", "113222", "123122", "123221", "223211", "221132",
    "221231", "213212", "223112", "312131", "311222", "321122", "321221", "312212", "322112", "322211",
    "212123", "212321", "232121", "111323", "131123", "131321", "112313", "132113", "132311", "211313",
    "231113", "231311", "112133", "112331", "132131", "113123", "113321", "133121", "313121", "211331",
    "231131", "213113", "213311", "213131", "311123", "311321", "331121", "312113", "312311", "332111",
    "314111", "221411", "431111", "111224", "111422", "121124", "121421", "141122", "141221", "112214",
    "112412", "122114", "122411", "142112", "142211", "241211", "221114", "413111", "241112", "134111",
    "111242", "121142", "121241", "114212", "124112", "124211", "411212", "421112", "421211", "212141",
    "214121", "412121", "111143", "111341", "131141", "114113", "114311", "411113", "411311", "113141",
    "114131", "311141", "411131", "211412", "211214", "211232", "2331112",
];
const START_B: u32 = 104;
const START_C: u32 = 105;
const CODE_B: u32 = 100;
const CODE_C: u32 = 99;
const STOP: usize = 106;

/// Valeurs des symboles du Code 128 (depart, donnees, controle) pour `text` : jeu C pour les series de chiffres (4 au moins),
/// jeu B pour le reste (ASCII 32 a 126).
pub fn code128_values(text: &str) -> Result<Vec<u32>, String> {
    if text.is_empty() { return Err(tr("The value is empty.").to_string()); }
    let b: Vec<u8> = text.bytes().collect();
    if let Some(c) = text.chars().find(|c| !(' '..='~').contains(c)) {
        return Err(trf!("Code 128 cannot encode the character “{}” (printable ASCII only).", c));
    }
    #[derive(PartialEq, Clone, Copy)]
    enum Set { None, B, C }
    let mut vals: Vec<u32> = Vec::new();
    let mut set = Set::None;
    let mut i = 0;
    while i < b.len() {
        let run = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if run >= 4 || (set == Set::C && run >= 2) {
            let mut run = run;
            if set != Set::C {
                if run % 2 == 1 {
                    // chiffre isole en jeu B avant de passer en C
                    if set == Set::None { vals.push(START_B); set = Set::B; }
                    vals.push((b[i] - 32) as u32);
                    i += 1; run -= 1;
                }
                if set == Set::None { vals.push(START_C); } else { vals.push(CODE_C); }
                set = Set::C;
            }
            while run >= 2 {
                vals.push(((b[i] - b'0') * 10 + (b[i + 1] - b'0')) as u32);
                i += 2; run -= 2;
            }
        } else {
            if set == Set::None { vals.push(START_B); } else if set == Set::C { vals.push(CODE_B); }
            set = Set::B;
            vals.push((b[i] - 32) as u32);
            i += 1;
        }
    }
    let sum: u32 = vals[0] + vals.iter().enumerate().skip(1).map(|(k, v)| k as u32 * v).sum::<u32>();
    vals.push(sum % 103);
    Ok(vals)
}

fn push_widths(out: &mut Vec<bool>, widths: &str) {
    let mut bar = true;
    for w in widths.bytes() {
        for _ in 0..(w - b'0') { out.push(bar); }
        bar = !bar;
    }
}

/// Code 128 de `text`.
pub fn code128(text: &str) -> Result<Barcode, String> {
    let vals = code128_values(text)?;
    let mut modules = Vec::new();
    for v in vals { push_widths(&mut modules, C128[v as usize]); }
    push_widths(&mut modules, C128[STOP]);
    Ok(Barcode { modules, text: text.to_string() })
}

const EAN_L: [&str; 10] = ["0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011", "0110111", "0001011"];
/// Parite des six chiffres de gauche selon le premier chiffre (`L` ou `G`).
const EAN_PARITY: [&str; 10] = ["LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG", "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL", "LGGLGL"];

fn bits(s: &str, out: &mut Vec<bool>) { out.extend(s.bytes().map(|c| c == b'1')); }
fn digit_l(d: usize) -> String { EAN_L[d].to_string() }
/// Jeu R : complement du jeu L ; jeu G : jeu R lu a l'envers.
fn digit_r(d: usize) -> String { EAN_L[d].bytes().map(|c| if c == b'1' { '0' } else { '1' }).collect() }
fn digit_g(d: usize) -> String { digit_r(d).chars().rev().collect() }

/// Chiffre de controle des chiffres `d` (poids 3 et 1 en alternance en partant de la droite).
pub fn check_digit(d: &[u8]) -> u8 {
    let s: u32 = d.iter().rev().enumerate().map(|(k, v)| *v as u32 * if k % 2 == 0 { 3 } else { 1 }).sum();
    ((10 - s % 10) % 10) as u8
}

/// Chiffres de `text` (espaces et tirets ignores), completes du chiffre de controle s'il manque (`n` chiffres sans, `n + 1` avec).
fn digits_with_check(text: &str, n: usize) -> Result<Vec<u8>, String> {
    let mut d: Vec<u8> = Vec::new();
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let Some(v) = c.to_digit(10) else { return Err(trf!("Only digits are allowed (found “{}”).", c)) };
        d.push(v as u8);
    }
    if d.len() == n { let c = check_digit(&d); d.push(c); Ok(d) }
    else if d.len() == n + 1 {
        let c = check_digit(&d[..n]);
        if d[n] != c { return Err(trf!("Wrong check digit: it should be {}.", c)); }
        Ok(d)
    } else { Err(trf!("{} or {} digits expected (found {}).", n, n + 1, d.len())) }
}

fn fmt_digits(d: &[u8]) -> String { d.iter().map(|v| (b'0' + v) as char).collect() }

/// EAN-13 (12 chiffres + controle) ; `upc` : UPC-A, soit un EAN-13 dont le premier chiffre est 0, lisible sur 12 chiffres.
fn ean13(d: &[u8], upc: bool) -> Barcode {
    let mut m = Vec::new();
    bits("101", &mut m);
    for (k, p) in EAN_PARITY[d[0] as usize].bytes().enumerate() {
        let x = d[k + 1] as usize;
        bits(&if p == b'L' { digit_l(x) } else { digit_g(x) }, &mut m);
    }
    bits("01010", &mut m);
    for k in 7..13 { bits(&digit_r(d[k] as usize), &mut m); }
    bits("101", &mut m);
    let s = fmt_digits(d);
    let text = if upc { format!("{} {} {} {}", &s[1..2], &s[2..7], &s[7..12], &s[12..]) } else { format!("{} {} {}", &s[..1], &s[1..7], &s[7..]) };
    Barcode { modules: m, text }
}

fn ean8(d: &[u8]) -> Barcode {
    let mut m = Vec::new();
    bits("101", &mut m);
    for k in 0..4 { bits(&digit_l(d[k] as usize), &mut m); }
    bits("01010", &mut m);
    for k in 4..8 { bits(&digit_r(d[k] as usize), &mut m); }
    bits("101", &mut m);
    let s = fmt_digits(d);
    Barcode { modules: m, text: format!("{} {}", &s[..4], &s[4..]) }
}

/// Encode `text` selon `kind`. Pour les codes EAN et UPC, le chiffre de controle est ajoute s'il manque et verifie s'il est donne.
pub fn encode(kind: BarcodeKind, text: &str) -> Result<Barcode, String> {
    match kind {
        BarcodeKind::Code128 => code128(text),
        BarcodeKind::Ean13 => Ok(ean13(&digits_with_check(text, 12)?, false)),
        BarcodeKind::Ean8 => Ok(ean8(&digits_with_check(text, 7)?)),
        BarcodeKind::UpcA => {
            let mut d = digits_with_check(text, 11)?;
            d.insert(0, 0);
            Ok(ean13(&d, true))
        }
    }
}

/// Barres du code : `(premier module, nombre de modules)` de chaque suite de barres noires.
pub fn bar_runs(modules: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < modules.len() {
        if modules[i] {
            let s = i;
            while i < modules.len() && modules[i] { i += 1; }
            runs.push((s, i - s));
        } else { i += 1; }
    }
    runs
}
