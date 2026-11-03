//! Traduction des textes du coeur (messages, noms affiches). Les textes du code sont en anglais et servent de cles ;
//! la table « anglais -> langue » est fournie par le programme (l'interface) avec `set_table`. Sans table, ou pour un texte
//! sans traduction, le texte anglais est renvoye tel quel.
use std::collections::HashMap;
use std::sync::RwLock;

use std::sync::atomic::{AtomicBool, Ordering};
static COMMA: AtomicBool = AtomicBool::new(false);

/// Virgule decimale pour `num` (langues qui l'utilisent, comme le francais).
pub fn set_decimal_comma(on: bool) {
    COMMA.store(on, Ordering::Relaxed);
}

/// Nombre deja formate avec un point decimal, ecrit avec la virgule si la langue l'utilise.
pub fn num(s: String) -> String {
    if COMMA.load(Ordering::Relaxed) { s.replace('.', ",") } else { s }
}

pub type Table = HashMap<&'static str, &'static str>;
static TABLE: RwLock<Option<Table>> = RwLock::new(None);

/// Remplace la table de traduction (`None` : anglais).
pub fn set_table(t: Option<Table>) {
    if let Ok(mut g) = TABLE.write() {
        *g = t;
    }
}

/// Texte traduit, ou le texte d'origine (anglais) s'il n'a pas de traduction.
pub fn tr(s: &'static str) -> &'static str {
    if let Ok(g) = TABLE.read() {
        if let Some(t) = g.as_ref().and_then(|m| m.get(s)) {
            return t;
        }
    }
    s
}

/// Texte avec valeurs : dans le modele (traduit), `{}` prend les valeurs dans l'ordre et `{0}`, `{1}`... une valeur precise.
pub fn format_tr(template: &'static str, args: &[String]) -> String {
    let t = tr(template);
    let mut out = String::with_capacity(t.len() + 16);
    let mut next = 0;
    let mut rest = t;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) => {
                let name = &after[..j];
                let idx = if name.is_empty() {
                    next += 1;
                    Some(next - 1)
                } else {
                    name.parse::<usize>().ok()
                };
                match idx.and_then(|k| args.get(k)) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `trf!("{} item(s)", n)` : comme `format!`, avec traduction du modele.
#[macro_export]
macro_rules! trf {
    ($t:expr $(, $v:expr)* $(,)?) => {
        $crate::i18n::format_tr($t, &[$(($v).to_string()),*])
    };
}

/// Charge une table « anglais -> langue » ecrite en JSON (objet de textes ; les cles commencant par `_` sont ignorees).
pub fn set_table_from_json(json: &str) {
    let mut t = Table::new();
    if let Ok(crate::json::Value::Obj(o)) = crate::json::parse(json) {
        for (k, v) in o {
            if let crate::json::Value::Str(v) = v {
                if !v.is_empty() && !k.starts_with('_') {
                    t.insert(Box::leak(k.into_boxed_str()), Box::leak(v.into_boxed_str()));
                }
            }
        }
    }
    set_table(if t.is_empty() { None } else { Some(t) });
}
