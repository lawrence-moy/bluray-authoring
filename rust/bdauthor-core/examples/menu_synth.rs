//! Menu de synthese (images unies, sans police) : `cargo run --example menu_synth -- sortie`
use bdauthor_core::menu::*;
use bdauthor_core::palette::RgbaImage;
use std::sync::Arc;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "menu_synth_out".into());
    std::fs::create_dir_all(&out).unwrap();
    let mk = |c: [u8; 3]| {
        let mut im = RgbaImage::new(480, 96);
        for y in 0..96 { for x in 0..480 { im.set(x, y, [c[0], c[1], c[2], if (x + y) % 97 < 90 { 255 } else { 0 }]); } }
        Arc::new(im)
    };
    let st = vec![mk([30, 60, 140]), mk([90, 150, 230]), mk([240, 150, 40])];
    let b: Vec<_> = (0..4).map(|i| MenuButton::new("b", i + 1, 240 + (i as i32 % 2) * 760, 300 + (i as i32 / 2) * 200, st.clone())).collect();
    let c = build_menu_clip(&b, &MenuOptions::default()).unwrap();
    std::fs::write(format!("{out}/80001.m2ts"), &c.m2ts).unwrap();
    std::fs::write(format!("{out}/80001.clpi"), &c.clpi).unwrap();
}
