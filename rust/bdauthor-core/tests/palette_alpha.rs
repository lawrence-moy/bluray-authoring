//! Palette des menus avec transparence : un halo garde un degrade fin (et non 4 paliers d'opacite).
use bdauthor_core::palette::*;
use std::collections::BTreeSet;

/// Bouton opaque (fond bleu, texte blanc adouci) entoure d'un halo de 0 a 230 d'opacite, comme le dessine le rendu.
fn halo_button(w: u32, h: u32, glow: u32) -> RgbaImage {
    let mut im = RgbaImage::new(w + 2 * glow, h + 2 * glow);
    for y in 0..im.height {
        for x in 0..im.width {
            let (dx, dy) = ((glow as i32 - x as i32).max(x as i32 - (glow + w - 1) as i32).max(0), (glow as i32 - y as i32).max(y as i32 - (glow + h - 1) as i32).max(0));
            let d = ((dx * dx + dy * dy) as f64).sqrt();
            if dx == 0 && dy == 0 {
                // texte : rayures blanches aux bords adoucis
                let t = ((x as f64 * 0.7).sin() * 0.5 + 0.5) * if (y as i32 - (glow + h / 2) as i32).abs() < 12 { 1.0 } else { 0.0 };
                let mix = |a: u8, b: u8| (a as f64 * (1.0 - t) + b as f64 * t).round() as u8;
                im.set(x, y, [mix(60, 255), mix(100, 255), mix(200, 255), 255]);
            } else if d < glow as f64 {
                let a = (230.0 * (1.0 - d / glow as f64).powf(1.5)).round() as u8;
                im.set(x, y, [120, 170, 255, a]);
            }
        }
    }
    im
}

fn halo_stats(im: &RgbaImage, q: &Quantized) -> (usize, f64) {
    let (entries, rows) = q;
    let rows = &rows[0];
    let (mut levels, mut err, mut n) = (BTreeSet::new(), 0.0, 0);
    for y in 0..im.height {
        for x in 0..im.width {
            let p = im.pixel(x, y);
            if p[3] > 0 && p[3] < 250 {
                let a = entries[&rows[y as usize][x as usize]].3;
                levels.insert(a);
                err += (a as f64 - p[3] as f64).abs();
                n += 1;
            }
        }
    }
    (levels.len(), err / n as f64)
}

#[test]
fn halo_keeps_a_fine_gradient() {
    let im = halo_button(400, 80, 24);
    let old = quantize_images(&[&im], 63, &ALPHA_LEVELS, ALPHA_MIN, true).unwrap();
    let new = quantize_rgba(&[&im], ALPHA_MIN);
    let (lo, eo) = halo_stats(&im, &old);
    let (ln, en) = halo_stats(&im, &new);
    println!("ancienne palette : {lo} niveaux, erreur moyenne {eo:.1} ; nouvelle : {ln} niveaux, erreur {en:.1}");
    assert!(lo <= 5);
    assert!(ln >= 20, "{ln} niveaux d'opacite seulement");
    assert!(en < eo / 4.0, "erreur {en:.1} contre {eo:.1}");
    // palette Blu-ray : au plus 254 entrees, indice 0 transparent, aucune entree visible sous le seuil
    assert!(new.0.len() <= MAX_ENTRIES);
    assert_eq!(new.0[&0].3, 0);
    assert!(new.0.iter().filter(|(i, _)| **i != 0).all(|(_, e)| e.3 >= ALPHA_MIN));
    // les pixels opaques restent opaques et les transparents transparents
    for y in 0..im.height {
        for x in 0..im.width {
            let p = im.pixel(x, y);
            let e = new.0[&new.1[0][y as usize][x as usize]];
            if p[3] == 255 { assert_eq!(e.3, 255); }
            if p[3] < ALPHA_MIN { assert_eq!(new.1[0][y as usize][x as usize], 0); }
        }
    }
}

#[test]
fn many_frames_share_one_palette() {
    // 8 images d'un halo qui pulse + des vignettes opaques : une seule palette, deterministe
    let frames: Vec<RgbaImage> = (0..8).map(|i| halo_button(300, 60, 8 + 3 * i)).collect();
    let mut thumb = RgbaImage::new(64, 64);
    for y in 0..64 { for x in 0..64 { thumb.set(x, y, [(x * 4) as u8, (y * 4) as u8, 128, 255]); } }
    let mut refs: Vec<&RgbaImage> = frames.iter().collect();
    refs.push(&thumb);
    let a = quantize_rgba(&refs, ALPHA_MIN);
    let b = quantize_rgba(&refs, ALPHA_MIN);
    assert_eq!(a, b);
    assert_eq!(a.1.len(), 9);
    for (k, f) in frames.iter().enumerate() {
        let (levels, err) = halo_stats(f, &(a.0.clone(), vec![a.1[k].clone()]));
        assert!(levels >= 12 && err < 6.0, "image {k} : {levels} niveaux, erreur {err:.1}");
    }
    // toutes opaques : quantification tramee des vignettes, inchangee
    let o = quantize_rgba(&[&thumb], ALPHA_MIN);
    assert!(o.0.values().skip(1).all(|e| e.3 == 255));
}

#[test]
#[ignore]
fn timing_busy_page() {
    // 12 vignettes « photo » (bruit) + boutons a halo anime : duree de la quantification
    let mut ims: Vec<RgbaImage> = Vec::new();
    let mut x: u32 = 12345;
    for _ in 0..12 {
        let mut t = RgbaImage::new(400, 225);
        for p in t.data.chunks_exact_mut(4) { x ^= x << 13; x ^= x >> 17; x ^= x << 5; p.copy_from_slice(&[x as u8, (x >> 8) as u8, (x >> 16) as u8, 255]); }
        ims.push(t);
    }
    for i in 0..8 { ims.push(halo_button(400, 80, 10 + 2 * i)); }
    let refs: Vec<&RgbaImage> = ims.iter().collect();
    let t0 = std::time::Instant::now();
    let q = quantize_rgba(&refs, ALPHA_MIN);
    println!("quantification : {:?}, {} entrees", t0.elapsed(), q.0.len());
}
