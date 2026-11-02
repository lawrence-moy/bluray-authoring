//! Traduction de l'interface. Les textes du code sont en anglais et servent de cles ; chaque langue fournit une table
//! « texte anglais -> texte traduit ». Un texte sans traduction s'affiche tel quel (en anglais).
//!
//! * `tr("Text")` : texte traduit (type `&'static str`, donc utilisable partout ou un litteral l'est) ;
//! * `trf!("{n} chapter(s)", n = count)` : texte avec valeurs ; chaque langue place les `{nom}` ou il faut ;
//! * le francais est integre (`lang/fr.json`) ; d'autres langues peuvent etre ajoutees sans recompiler : un fichier
//!   `<code>.json` dans le dossier `langues` a cote du programme (cle `"_name"` = nom affiche dans la liste).
use bdauthor_core::json::{parse, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::RwLock;

const FR_JSON: &str = include_str!("../lang/fr.json");

type Table = HashMap<&'static str, &'static str>;
static TABLE: RwLock<Option<Table>> = RwLock::new(None);
static CODE: RwLock<String> = RwLock::new(String::new());

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Texte traduit dans la langue choisie, ou le texte d'origine (anglais) s'il n'a pas de traduction.
pub fn tr(s: &'static str) -> &'static str {
    if let Ok(g) = TABLE.read() {
        if let Some(t) = g.as_ref().and_then(|m| m.get(s)) {
            return t;
        }
    }
    s
}

/// Texte avec valeurs : dans le modele (traduit), `{}` prend les valeurs dans l'ordre et `{0}`, `{1}`... une valeur precise
/// (utile quand une langue change l'ordre des mots).
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

macro_rules! trf {
    ($t:expr $(, $v:expr)* $(,)?) => {
        $crate::i18n::format_tr($t, &[$(($v).to_string()),*])
    };
}
pub(crate) use trf;

/// Dossier des langues ajoutees par l'utilisateur : `langues` a cote du programme.
pub fn user_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.join("langues"))
}

fn read_table(json: &str, into: &mut Table) {
    if let Ok(Value::Obj(o)) = parse(json) {
        for (k, v) in o {
            if let Value::Str(t) = v {
                if !t.is_empty() && !k.starts_with('_') {
                    into.insert(leak(k), leak(t));
                }
            }
        }
    }
}

/// Choisit la langue (`"en"` : textes du code, `"fr"` : francais integre, autre : fichier du dossier `langues`).
pub fn set_language(code: &str) {
    let mut t = Table::new();
    if code == "fr" {
        read_table(FR_JSON, &mut t);
    }
    if let Some(d) = user_dir() {
        if let Ok(s) = std::fs::read_to_string(d.join(format!("{code}.json"))) {
            read_table(&s, &mut t);
        }
    }
    if let Ok(mut g) = TABLE.write() {
        *g = if t.is_empty() { None } else { Some(t) };
    }
    if let Ok(mut c) = CODE.write() {
        *c = code.to_string();
    }
}

/// Nombre ecrit avec la virgule decimale en francais.
pub fn num(s: String) -> String {
    if current() == "fr" { s.replace('.', ",") } else { s }
}

pub fn current() -> String {
    CODE.read().map(|c| c.clone()).unwrap_or_default()
}

/// Langues disponibles : (code, nom affiche).
pub fn available() -> Vec<(String, String)> {
    let mut out = vec![("en".to_string(), "English".to_string()), ("fr".to_string(), "Français".to_string())];
    if let Some(d) = user_dir() {
        if let Ok(rd) = std::fs::read_dir(d) {
            let mut extra: Vec<(String, String)> = rd
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) != Some("json") {
                        return None;
                    }
                    let code = p.file_stem()?.to_string_lossy().into_owned();
                    let name = std::fs::read_to_string(&p).ok().and_then(|s| parse(&s).ok()).and_then(|v| v.get("_name").and_then(Value::as_str).map(String::from)).unwrap_or_else(|| code.clone());
                    Some((code, name))
                })
                .filter(|(c, _)| c != "en" && c != "fr")
                .collect();
            extra.sort();
            out.extend(extra);
        }
    }
    out
}

/// Modele de traduction : tous les textes de l'interface (cles anglaises) avec une valeur vide, a remplir puis a enregistrer
/// dans `langues/<code>.json`.
pub fn template(name: &str) -> String {
    let keys: BTreeSet<String> = match parse(FR_JSON) {
        Ok(Value::Obj(o)) => o.into_iter().map(|(k, _)| k).filter(|k| !k.starts_with('_')).collect(),
        _ => BTreeSet::new(),
    };
    let mut o = vec![("_name".to_string(), Value::Str(name.to_string()))];
    o.extend(keys.into_iter().map(|k| (k, Value::Str(String::new()))));
    Value::Obj(o).to_pretty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_in_order_and_by_index() {
        let a = vec!["2".to_string(), "s".to_string(), "cut".to_string()];
        assert_eq!(format_tr("{} widget{} {}.", &a), "2 widgets cut.");
        assert_eq!(format_tr("{2} {0}{1}", &a), "cut 2s");
        assert_eq!(format_tr("{} {}", &a[..1]), "2 {}");
    }

    #[test]
    fn french_table_parses_and_has_no_empty_values() {
        let mut t = Table::new();
        read_table(FR_JSON, &mut t);
        assert!(t.len() > 500);
        assert!(t.values().all(|v| !v.is_empty()));
    }
}
