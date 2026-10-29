//! Conversion d'un ancien projet (pages automatiques du menu principal, pages libres, menus de chapitres) en pages generiques :
//! toutes les pages deviennent des pages personnalisees, les vignettes montrent une video choisie ou un chapitre (instant de depart).
use crate::design::*;
use crate::project::paginate;

/// Une video du projet pour la conversion : son titre et ses chapitres (instant en secondes, nom), vide s'il y en a moins de 2.
#[derive(Debug, Clone, PartialEq)]
pub struct GenVideo {
    pub title: String,
    pub chapters: Vec<(f64, String)>,
}

fn mmss(t: f64) -> String {
    let s = t.round().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Pages generiques equivalentes a l'ancien menu : pages du menu principal (personnalisees ou grille automatique), pages libres,
/// puis menus de chapitres de chaque video. Style et accueil sont repris ; les destinations sont renumerotees.
pub fn generate_pages(videos: &[GenVideo], par_page: Option<usize>, disc_title: &str, legacy: &Designs) -> Result<Designs, String> {
    let n = videos.len();
    let mut out = Designs { style: legacy.style.clone(), generic: true, ..Default::default() };
    if n == 0 {
        return Ok(out);
    }
    let main = paginate(n, par_page).map_err(|e| e.to_string())?;
    let (m, f) = (main.len(), legacy.free.len());
    // premiere page de chapitres de chaque video, et pagination de ses chapitres
    let mut ch_first: Vec<Option<usize>> = vec![None; n];
    let mut ch_pages: Vec<Vec<Vec<usize>>> = vec![Vec::new(); n];
    let mut next = m + f;
    for (v, gv) in videos.iter().enumerate() {
        if gv.chapters.len() >= 2 {
            let pg = paginate(gv.chapters.len(), par_page).map_err(|e| e.to_string())?;
            ch_first[v] = Some(next);
            next += pg.len();
            ch_pages[v] = pg;
        }
    }
    let total = next;
    let video_page = |v: usize| main.iter().position(|g| g.contains(&v)).unwrap_or(0);
    let remap = |t: Target| -> Option<Target> {
        match t {
            Target::Main(p) => (p < m + f).then_some(Target::Main(p)),
            Target::Chapters(v, q) => ch_first.get(v).copied().flatten().filter(|_| q < ch_pages[v].len()).map(|i| Target::Main(i + q)),
            Target::Video(v) => (v < n).then_some(Target::Video(v)),
        }
    };
    let fix_links = |d: &mut PageDesign| {
        for it in &mut d.items {
            if let Some(t) = it.targets_mut() {
                *t = t.and_then(remap);
            }
            // une vignette de video a chapitres ouvrait son menu de chapitres
            if let Kind::Slot { video: Some(Some(v)), link, start: None, .. } = &mut it.kind {
                if link.is_none() {
                    if let Some(i) = ch_first.get(*v).copied().flatten() { *link = Some(Target::Main(i)); }
                }
            }
        }
    };
    let pages_label = |base: String, q: usize, of: usize| if of > 1 { format!("{base} ({}/{of})", q + 1) } else { base };

    // menu principal
    for (p, idxs) in main.iter().enumerate() {
        let mut d = match legacy.main.get(&p) {
            Some(d) => d.clone(),
            None => PageDesign::starting_point(MenuType::Main, idxs.len(), p, m + f, disc_title)?,
        };
        let asg = d.slot_assignment(idxs.len());
        let mut k = 0;
        for it in &mut d.items {
            if let Kind::Slot { video: video @ None, assign, .. } = &mut it.kind {
                let a = asg.get(k).copied().unwrap_or(usize::MAX);
                k += 1;
                *video = Some(idxs.get(a).copied());
                *assign = None;
            }
        }
        fix_links(&mut d);
        if d.name.is_empty() { d.name = pages_label("Menu principal".into(), p, m); }
        out.free.push(d);
    }
    // pages libres
    for (k, d) in legacy.free.iter().enumerate() {
        let mut d = d.clone();
        fix_links(&mut d);
        if d.name.is_empty() { d.name = format!("Page libre {}", k + 1); }
        out.free.push(d);
    }
    // menus de chapitres
    for (v, gv) in videos.iter().enumerate() {
        let npg = ch_pages[v].len();
        for (q, idxs) in ch_pages[v].iter().enumerate() {
            let mut d = match legacy.chapters.get(&(v, q)) {
                Some(d) => d.clone(),
                None => PageDesign::starting_point(MenuType::Chapters, idxs.len(), q, npg, &format!("{} - chapitres", gv.title))?,
            };
            let asg = d.slot_assignment(idxs.len());
            let mut k = 0;
            for it in &mut d.items {
                match &mut it.kind {
                    Kind::Slot { video: video @ None, assign, start, label, .. } => {
                        let a = asg.get(k).copied().unwrap_or(usize::MAX);
                        k += 1;
                        *assign = None;
                        match idxs.get(a).and_then(|&j| gv.chapters.get(j)) {
                            Some((t, name)) => {
                                *video = Some(Some(v));
                                *start = Some(*t);
                                *label = format!("{name} ({})", mmss(*t));
                            }
                            None => *video = Some(None),
                        }
                    }
                    Kind::Nav { action, target, .. } if *action == NavAction::PlayAll => {
                        *action = NavAction::Goto;
                        *target = Some(Target::Video(v));
                    }
                    Kind::Nav { action, target, .. } if *action == NavAction::Back => {
                        *action = NavAction::Goto;
                        *target = Some(Target::Main(video_page(v)));
                    }
                    _ => {}
                }
            }
            fix_links(&mut d);
            if d.name.is_empty() { d.name = pages_label(format!("Chapitres : {}", gv.title), q, npg); }
            out.free.push(d);
        }
    }
    debug_assert_eq!(out.free.len(), total);
    out.home = legacy.home.and_then(remap).filter(|t| !matches!(t, Target::Video(_)));
    Ok(out)
}
