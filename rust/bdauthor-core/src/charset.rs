//! Pages de code des fichiers de sous-titres (sans dependance externe : tables generees dans `charset_tables.rs` et `charset_data/`).
use crate::charset_tables as t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// UTF-8 si le fichier en est, sinon la page de code habituelle de la langue de la piste (Windows-1252 a defaut).
    Auto,
    Utf8,
    Cp1252,
    Cp1250,
    Cp1251,
    Cp1253,
    Cp1254,
    Cp1255,
    Cp1256,
    Cp1257,
    Cp1258,
    Cp874,
    ShiftJis,
    Gbk,
    Big5,
    Uhc,
}

impl Charset {
    pub const ALL: [Charset; 16] = [
        Charset::Auto, Charset::Utf8, Charset::Cp1252, Charset::Cp1250, Charset::Cp1251, Charset::Cp1253, Charset::Cp1254, Charset::Cp1255,
        Charset::Cp1256, Charset::Cp1257, Charset::Cp1258, Charset::Cp874, Charset::ShiftJis, Charset::Gbk, Charset::Big5, Charset::Uhc,
    ];

    /// Nom dans le fichier de projet.
    pub fn name(self) -> &'static str {
        match self {
            Charset::Auto => "auto", Charset::Utf8 => "utf-8", Charset::Cp1252 => "1252", Charset::Cp1250 => "1250", Charset::Cp1251 => "1251",
            Charset::Cp1253 => "1253", Charset::Cp1254 => "1254", Charset::Cp1255 => "1255", Charset::Cp1256 => "1256", Charset::Cp1257 => "1257",
            Charset::Cp1258 => "1258", Charset::Cp874 => "874", Charset::ShiftJis => "shift-jis", Charset::Gbk => "gbk", Charset::Big5 => "big5", Charset::Uhc => "949",
        }
    }

    pub fn from_name(s: &str) -> Option<Charset> {
        Charset::ALL.into_iter().find(|c| c.name() == s)
    }

    /// Libelle affiche.
    pub fn label(self) -> &'static str {
        match self {
            Charset::Auto => "Automatique", Charset::Utf8 => "UTF-8", Charset::Cp1252 => "Europe de l'Ouest (Windows-1252)",
            Charset::Cp1250 => "Europe centrale (Windows-1250)", Charset::Cp1251 => "Cyrillique (Windows-1251)", Charset::Cp1253 => "Grec (Windows-1253)",
            Charset::Cp1254 => "Turc (Windows-1254)", Charset::Cp1255 => "Hébreu (Windows-1255)", Charset::Cp1256 => "Arabe (Windows-1256)",
            Charset::Cp1257 => "Baltique (Windows-1257)", Charset::Cp1258 => "Vietnamien (Windows-1258)", Charset::Cp874 => "Thaï (Windows-874)",
            Charset::ShiftJis => "Japonais (Shift-JIS)", Charset::Gbk => "Chinois simplifié (GBK)", Charset::Big5 => "Chinois traditionnel (Big5)", Charset::Uhc => "Coréen (CP949)",
        }
    }

    /// Page de code usuelle des fichiers non UTF-8 d'une langue (code de 3 lettres).
    pub fn for_language(code: &str) -> Charset {
        match code.to_ascii_lowercase().as_str() {
            "pol" | "ces" | "cze" | "slk" | "slo" | "hun" | "ron" | "rum" | "hrv" | "slv" | "sqi" | "alb" | "bos" => Charset::Cp1250,
            "rus" | "ukr" | "bul" | "srp" | "bel" | "mkd" | "kaz" => Charset::Cp1251,
            "ell" | "gre" => Charset::Cp1253,
            "tur" | "aze" => Charset::Cp1254,
            "heb" => Charset::Cp1255,
            "ara" | "fas" | "per" | "urd" => Charset::Cp1256,
            "est" | "lav" | "lit" => Charset::Cp1257,
            "vie" => Charset::Cp1258,
            "tha" => Charset::Cp874,
            "jpn" => Charset::ShiftJis,
            "zho" | "chi" => Charset::Gbk,
            "kor" => Charset::Uhc,
            _ => Charset::Cp1252,
        }
    }
}

fn single_byte(b: &[u8], table: &[u16; 128]) -> String {
    b.iter().map(|&c| if c < 0x80 { c as char } else { char::from_u32(table[(c - 0x80) as usize] as u32).unwrap_or('\u{FFFD}') }).collect()
}

/// Page de code a un ou deux octets : `leads` = plages de premier octet, `trail` = plage du second ; la table donne un caractere par couple.
fn double_byte(b: &[u8], table: &[u8], leads: &[(u8, u8)], trail: (u8, u8), single: &dyn Fn(u8) -> Option<char>) -> String {
    let ntrail = (trail.1 - trail.0) as usize + 1;
    let lead_index = |c: u8| -> Option<usize> {
        let mut base = 0usize;
        for &(lo, hi) in leads {
            if c >= lo && c <= hi {
                return Some(base + (c - lo) as usize);
            }
            base += (hi - lo) as usize + 1;
        }
        None
    };
    let mut out = String::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < 0x80 {
            out.push(c as char);
            i += 1;
        } else if let Some(ch) = single(c) {
            out.push(ch);
            i += 1;
        } else if let (Some(l), Some(&tb)) = (lead_index(c), b.get(i + 1)) {
            if tb >= trail.0 && tb <= trail.1 {
                let k = (l * ntrail + (tb - trail.0) as usize) * 2;
                let v = u16::from_le_bytes([table[k], table[k + 1]]);
                out.push(char::from_u32(v as u32).unwrap_or('\u{FFFD}'));
                i += 2;
            } else {
                out.push('\u{FFFD}');
                i += 1;
            }
        } else {
            out.push('\u{FFFD}');
            i += 1;
        }
    }
    out
}

/// Decode des octets selon une page de code d'ancienne generation (jamais `Auto` ni `Utf8`).
fn legacy(b: &[u8], cs: Charset) -> String {
    match cs {
        Charset::Cp1250 => single_byte(b, &t::CP1250),
        Charset::Cp1251 => single_byte(b, &t::CP1251),
        Charset::Cp1253 => single_byte(b, &t::CP1253),
        Charset::Cp1254 => single_byte(b, &t::CP1254),
        Charset::Cp1255 => single_byte(b, &t::CP1255),
        Charset::Cp1256 => single_byte(b, &t::CP1256),
        Charset::Cp1257 => single_byte(b, &t::CP1257),
        Charset::Cp1258 => single_byte(b, &t::CP1258),
        Charset::Cp874 => single_byte(b, &t::CP874),
        Charset::ShiftJis => double_byte(b, include_bytes!("charset_data/sjis.bin"), &[(0x81, 0x9F), (0xE0, 0xFC)], (0x40, 0xFC), &|c| match c {
            0xA1..=0xDF => char::from_u32(0xFF61 + (c - 0xA1) as u32),
            0x80 | 0xA0 | 0xFD..=0xFF => Some('\u{FFFD}'),
            _ => None,
        }),
        Charset::Gbk => double_byte(b, include_bytes!("charset_data/gbk.bin"), &[(0x81, 0xFE)], (0x40, 0xFE), &|c| match c {
            0x80 => Some('\u{20AC}'),
            0xFF => Some('\u{FFFD}'),
            _ => None,
        }),
        Charset::Big5 => double_byte(b, include_bytes!("charset_data/big5.bin"), &[(0x81, 0xFE)], (0x40, 0xFE), &|c| if c == 0x80 || c == 0xFF { Some('\u{FFFD}') } else { None }),
        Charset::Uhc => double_byte(b, include_bytes!("charset_data/uhc.bin"), &[(0x81, 0xFE)], (0x41, 0xFE), &|c| if c == 0x80 || c == 0xFF { Some('\u{FFFD}') } else { None }),
        _ => single_byte(b, &t::CP1252),
    }
}

/// Octets d'un fichier texte -> chaine. UTF-16 avec BOM et UTF-8 avec BOM sont toujours reconnus.
/// `cs` : page de code imposee ; `Auto` : UTF-8 si le contenu en est, sinon `hint` (page de code de la langue ; Windows-1252 si `Auto`/`Utf8`).
pub fn decode(b: &[u8], cs: Charset, hint: Charset) -> String {
    if b.starts_with(&[0xFF, 0xFE]) || b.starts_with(&[0xFE, 0xFF]) {
        let le = b[0] == 0xFF;
        let u: Vec<u16> = b[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        return String::from_utf16_lossy(&u);
    }
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    match cs {
        Charset::Utf8 => String::from_utf8_lossy(b).into_owned(),
        Charset::Auto => match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            Err(_) => legacy(b, if matches!(hint, Charset::Auto | Charset::Utf8) { Charset::Cp1252 } else { hint }),
        },
        other => legacy(b, other),
    }
}
