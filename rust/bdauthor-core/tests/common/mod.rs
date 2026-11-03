//! Les messages du coeur sont en anglais ; ces essais verifient les textes francais.
#![allow(dead_code)]
pub fn fr() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| bdauthor_core::i18n::set_table_from_json(include_str!("../../../bdauthor-gui/lang/fr.json")));
}
