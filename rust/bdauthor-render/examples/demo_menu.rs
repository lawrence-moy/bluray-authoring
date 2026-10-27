//! Genere un clip de menu IG de demonstration (texte) : `cargo run --release --example demo_menu -- sortie`
//! Produit sortie/80001.m2ts et sortie/80001.clpi ; verifier avec `bdinspect` du crate bdauthor-core.
use bdauthor_core::menu::*;
use bdauthor_render::*;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "demo_menu_out".into());
    std::fs::create_dir_all(&out).unwrap();
    let font = load_font(&find_font().expect("aucune police : definir BDAUTHOR_FONT")).unwrap();
    let mut buttons = Vec::new();
    for (i, label) in ["Film 1", "Film 2", "Film 3", "Film 4"].iter().enumerate() {
        let (x, y) = (240 + (i as i32 % 2) * 760, 300 + (i as i32 / 2) * 200);
        buttons.push(MenuButton::new(label, i as u32 + 1, x, y, text_button_states(&font, label, 480, 96)));
    }
    let clip = build_menu_clip(&buttons, &MenuOptions::default()).unwrap();
    std::fs::write(format!("{out}/80001.m2ts"), &clip.m2ts).unwrap();
    std::fs::write(format!("{out}/80001.clpi"), &clip.clpi).unwrap();
    println!("{} paquets, {} objets -> {out}", clip.m2ts.len() / 192, clip.menu.ods.len());
}
