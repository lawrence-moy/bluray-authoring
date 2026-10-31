//! Pages de depart d'un projet : a partir des videos, une grille de vignettes (sur une ou plusieurs pages) puis, pour chaque video
//! qui a des chapitres, ses pages de chapitres. Toutes sont des pages ordinaires, modifiables ensuite dans l'editeur.
use crate::design::*;
use crate::project::{layout, paginate};
use crate::style::ButtonStyle;

/// Une video du projet : son titre et ses chapitres (instant en secondes, nom), vide s'il y en a moins de 2.
#[derive(Debug, Clone, PartialEq)]
pub struct GenVideo {
    pub title: String,
    pub chapters: Vec<(f64, String)>,
}

fn mmss(t: f64) -> String {
    let s = t.round().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Grille de vignettes (une par entree : video, instant de depart, libelle), en reservant `reserve` px en bas pour les boutons.
fn grid(entries: &[(usize, Option<f64>, String)], reserve: i32) -> Result<Vec<Item>, String> {
    Ok(layout(entries.len(), (1920, 1080), reserve).map_err(|e| e.to_string())?
        .into_iter().zip(entries).map(|(c, (v, start, label))| Item::video_slot(c.x, c.y, c.w, c.h, c.th, *v, *start, label)).collect())
}

fn goto(text: &str, t: Target, x: i32, w: i32) -> Item {
    let mut it = Item::nav(NavAction::Goto, x, NAV_Y, w, 80);
    if let Kind::Nav { text: tx, target, .. } = &mut it.kind { *tx = text.to_string(); *target = Some(t); }
    it
}

/// Pages de depart pour ces videos : grille des videos (une vignette de video a chapitres mene a ses chapitres), puis pages de
/// chapitres (« Tout lire » lit la video, « Retour » revient a la page de la video). Le style du projet est repris.
pub fn generate_pages(videos: &[GenVideo], disc_title: &str, style: &ButtonStyle) -> Result<Designs, String> {
    let n = videos.len();
    let mut out = Designs { style: style.clone(), ..Default::default() };
    if n == 0 {
        return Ok(out);
    }
    let main = paginate(n, None).map_err(|e| e.to_string())?;
    let m = main.len();
    // premiere page de chapitres de chaque video, et pagination de ses chapitres
    let mut ch_first: Vec<Option<usize>> = vec![None; n];
    let mut ch_pages: Vec<Vec<Vec<usize>>> = vec![Vec::new(); n];
    let mut next = m;
    for (v, gv) in videos.iter().enumerate() {
        if gv.chapters.len() >= 2 {
            let pg = paginate(gv.chapters.len(), None).map_err(|e| e.to_string())?;
            ch_first[v] = Some(next);
            next += pg.len();
            ch_pages[v] = pg;
        }
    }
    let video_page = |v: usize| main.iter().position(|g| g.contains(&v)).unwrap_or(0);
    let pages_label = |base: String, q: usize, of: usize| if of > 1 { format!("{base} ({}/{of})", q + 1) } else { base };

    // grille des videos
    for (p, idxs) in main.iter().enumerate() {
        let entries: Vec<(usize, Option<f64>, String)> = idxs.iter().map(|&v| (v, None, String::new())).collect();
        let mut items = grid(&entries, if m > 1 { 140 } else { 0 })?;
        for it in &mut items {
            if let Kind::Slot { video: Some(v), link, .. } = &mut it.kind {
                if let Some(i) = ch_first[*v] { *link = Some(Target::Main(i)); }
            }
        }
        items.insert(0, Item::text(disc_title, 100, 90, 1720, 110, 84));
        if p > 0 { items.push(Item::nav(NavAction::Prev, 560, NAV_Y, 360, 80)); }
        if p + 1 < m { items.push(Item::nav(NavAction::Next, 1000, NAV_Y, 360, 80)); }
        out.free.push(PageDesign { items, name: pages_label("Vidéos".into(), p, m), ..Default::default() });
    }
    // pages de chapitres
    for (v, gv) in videos.iter().enumerate() {
        let npg = ch_pages[v].len();
        for (q, idxs) in ch_pages[v].iter().enumerate() {
            let entries: Vec<(usize, Option<f64>, String)> = idxs.iter().filter_map(|&j| gv.chapters.get(j)).map(|(t, name)| (v, Some(*t), format!("{name} ({})", mmss(*t)))).collect();
            let mut items = grid(&entries, 140)?;
            items.insert(0, Item::text(&format!("{} - chapitres", gv.title), 100, 90, 1720, 110, 84));
            if npg == 1 {
                items.push(goto("Tout lire", Target::Video(v), 560, 360));
                items.push(goto("Retour", Target::Main(video_page(v)), 1000, 360));
            } else {
                if q > 0 { items.push(Item::nav(NavAction::Prev, 260, NAV_Y, 320, 80)); }
                items.push(goto("Tout lire", Target::Video(v), 620, 320));
                items.push(goto("Retour", Target::Main(video_page(v)), 980, 320));
                if q + 1 < npg { items.push(Item::nav(NavAction::Next, 1340, NAV_Y, 320, 80)); }
            }
            out.free.push(PageDesign { items, name: pages_label(format!("Chapitres : {}", gv.title), q, npg), ..Default::default() });
        }
    }
    debug_assert_eq!(out.free.len(), next);
    Ok(out)
}
