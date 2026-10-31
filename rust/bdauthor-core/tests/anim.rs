//! Halo anime : style, images de l'animation, ecriture dans le menu (objets consecutifs, boucle, cadence).
use bdauthor_core::menu::{anim_rate_code, build_menu, MenuButton, MenuOptions};
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::style::{ButtonStyle, PULSE_FRAMES};
use std::sync::Arc;

fn img(w: u32, h: u32, c: [u8; 4]) -> Arc<RgbaImage> {
    Arc::new(RgbaImage { width: w, height: h, data: c.iter().copied().cycle().take((w * h * 4) as usize).collect() })
}

#[test]
fn pulse_style_and_json() {
    let st = ButtonStyle { glow: 20, glow_pulse: true, ..ButtonStyle::default() };
    assert_eq!(st.pulse_frames(), PULSE_FRAMES);
    assert_eq!(st.state_count(), 2 + PULSE_FRAMES as u64 - 1);
    // image 0 = halo complet ; le plus petit a mi-parcours (35 %) ; retour symetrique
    let g: Vec<i32> = (0..PULSE_FRAMES).map(|i| st.pulse_style(i).glow).collect();
    assert_eq!(g[0], 20);
    assert_eq!(g[PULSE_FRAMES / 2], 7);
    assert_eq!(g[1], g[PULSE_FRAMES - 1]);
    // sans halo : pas d'animation
    assert_eq!(ButtonStyle { glow: 0, glow_pulse: true, ..ButtonStyle::default() }.pulse_frames(), 1);
    let back = ButtonStyle::from_value(&st.to_value()).unwrap();
    assert!(back.glow_pulse);
    assert!(!ButtonStyle::from_value(&ButtonStyle::default().to_value()).unwrap().glow_pulse);
}

#[test]
fn animated_selected_state_in_menu() {
    let (n, s, a) = (img(40, 20, [10, 10, 10, 255]), img(40, 20, [200, 30, 30, 255]), img(40, 20, [30, 200, 30, 255]));
    let mut b0 = MenuButton::new("", 1, 100, 100, vec![n.clone(), s.clone(), a.clone()]);
    b0.sel_frames = (1..4).map(|k| img(40, 20, [200, 30 + 40 * k, 30, 255])).collect();
    let b1 = MenuButton::new("", 2, 300, 100, vec![n.clone(), s.clone(), a.clone()]);
    let o = MenuOptions { frame_rate_code: 3, ..Default::default() };
    let m = build_menu(&[b0, b1], &o).unwrap();
    let page = &m.ics.pages[0];
    assert_eq!(page.frame_rate_code, anim_rate_code(3));
    assert_eq!(anim_rate_code(3), 2); // 25 i/s : une image sur 3 (8,3 i/s)
    let btn = |i: usize| &page.bogs[i].buttons[0];
    // bouton anime : 4 objets consecutifs, en boucle ; le bouton fixe partage toujours ses images
    let sel = btn(0).selected;
    assert_eq!((sel.2 - sel.1, sel.3), (3, 0x80));
    assert_eq!(btn(1).selected.1, btn(1).selected.2);
    assert_eq!(btn(1).selected.3, 0);
    assert!(m.ods.iter().any(|o| o.object_id == sel.2));
    // sans animation : pas de cadence
    let plain = build_menu(&[MenuButton::new("", 1, 0, 0, vec![n, s, a])], &o).unwrap();
    assert_eq!(plain.ics.pages[0].frame_rate_code, 0);
    // relecture : le format ecrit se relit a l'identique
    let packed = m.ics.pack();
    let re = bdauthor_core::ig::Ics::parse(&packed).unwrap();
    assert_eq!(re.pages[0].bogs[0].buttons[0].selected, sel);
}
