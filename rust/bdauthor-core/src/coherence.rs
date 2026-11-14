//! Verificateur de coherence des menus : le projet vu comme un graphe (pages, boutons, videos, retours apres lecture).
//!
//! Complete les controles par page (`PageDesign::validate`) et ceux du dossier BDMV (`check`) par ce qui ne se voit qu'a
//! l'echelle du disque : pages inaccessibles depuis l'accueil, videos qu'aucun bouton ne lance, retours vers une page supprimee,
//! boucles de lancement automatique, boutons injoignables a la telecommande, chapitres, langues, boutons en double.
//! Les boutons et leurs voisins sont ceux du disque (`editor::sim_buttons`, meme regle que le constructeur).
use crate::design::{SlotReturn, SubChoice, Target};
use crate::editor::{sim_buttons, video_title, PageInfo, PageRef, SimAction, SimButton};
use crate::i18n::tr;
use crate::model::ProjectModel;
use crate::trf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity { Error, Warning, Info }

/// Regle qui a produit un constat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule { Reach, Orphan, Return, AutoLoop, Arrows, Chapters, Languages, Duplicates }

impl Rule {
    pub fn label(self) -> &'static str {
        match self {
            Rule::Reach => tr("accessibility"),
            Rule::Orphan => tr("orphan videos"),
            Rule::Return => tr("return after playback"),
            Rule::AutoLoop => tr("automatic launch"),
            Rule::Arrows => tr("arrow navigation"),
            Rule::Chapters => tr("chapters"),
            Rule::Languages => tr("languages"),
            Rule::Duplicates => tr("duplicates"),
        }
    }
}

/// Ce que vise un constat : une page, un element d'une page (indice dans `PageDesign::items`), une video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place { Page(usize), Item(usize, usize), Video(usize) }

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    pub rule: Rule,
    pub msg: String,
    pub place: Place,
}

/// Ou mene un bouton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest { Page(usize), Video(usize), PlayAll }

/// Sortie d'une page (carte du disque) ; `auto_s` : lancee automatiquement au bout de ce delai.
#[derive(Debug, Clone, PartialEq)]
pub struct Exit { pub dest: Dest, pub auto_s: Option<u32> }

#[derive(Debug, Clone, PartialEq)]
pub struct PageNode { pub name: String, pub reachable: bool, pub exits: Vec<Exit> }

/// Video sur la carte : pages qui la lancent (« Tout lire » compris) et pages ou l'on revient a la fin.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoNode { pub title: String, pub from: Vec<usize>, pub returns: Vec<usize> }

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    /// Du plus grave au moins grave.
    pub findings: Vec<Finding>,
    pub pages: Vec<PageNode>,
    pub videos: Vec<VideoNode>,
    /// page d'accueil (ouverture du disque et touche Menu)
    pub home: usize,
    /// aucune page : le menu sera genere a la construction
    pub generated: bool,
}

impl Report {
    pub fn count(&self, s: Severity) -> usize { self.findings.iter().filter(|f| f.severity == s).count() }
}

fn mmss(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Nom d'une page : le sien, sinon « Page N ».
pub fn page_name(m: &ProjectModel, k: usize) -> String {
    match m.designs.free.get(k).map(|d| d.name.trim()).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => trf!("Page {}", k + 1),
    }
}

/// Nom d'un element, entre guillemets : le sien, son texte, sa video, sinon « element N ».
pub fn item_name(m: &ProjectModel, page: usize, idx: usize) -> String {
    use crate::design::Kind;
    let Some(it) = m.designs.free.get(page).and_then(|d| d.items.get(idx)) else { return trf!("item {}", idx + 1) };
    let short = |s: &str| {
        let line = s.lines().next().unwrap_or("").trim();
        if line.chars().count() > 40 { format!("{}…", line.chars().take(39).collect::<String>()) } else { line.to_string() }
    };
    let name = if !it.name.trim().is_empty() {
        short(&it.name)
    } else {
        match &it.kind {
            Kind::Nav { text, action, .. } => if text.trim().is_empty() { action.default_text().to_string() } else { short(text) },
            Kind::Text { text, .. } => short(text),
            Kind::Slot { label, video, start, .. } if label.trim().is_empty() => match (video, start) {
                (Some(v), Some(t)) if *v < m.videos.len() => format!("{} {}", video_title(m, *v), mmss(*t)),
                (Some(v), None) if *v < m.videos.len() => video_title(m, *v),
                _ => String::new(),
            },
            Kind::Slot { label, .. } => short(label),
            _ => String::new(),
        }
    };
    if name.is_empty() { trf!("item {}", idx + 1) } else { format!("« {name} »") }
}

fn video_name(m: &ProjectModel, v: usize) -> String { trf!("video {} « {} »", v + 1, video_title(m, v)) }

/// Liste lisible de videos (« les videos 2 et 3 »).
fn videos_list(v: &[usize]) -> String {
    let n: Vec<String> = v.iter().map(|x| (x + 1).to_string()).collect();
    match n.len() {
        0 => String::new(),
        1 => trf!("video {}", n[0]),
        _ => trf!("videos {} and {}", n[..n.len() - 1].join(", "), n[n.len() - 1]),
    }
}

/// Effet d'un bouton : ou il mene, et la page affichee a la fin de la lecture qu'il lance.
fn effect(b: &SimButton, k: usize, np: usize, home: usize, n: usize) -> (Option<Dest>, Option<usize>) {
    let back = match b.ret { SlotReturn::Origin => k, SlotReturn::Home => home, SlotReturn::Page(q) if q < np => q, SlotReturn::Page(_) => k };
    let page = |q: usize| (q < np).then_some(Dest::Page(q));
    match b.action {
        SimAction::Play(v) | SimAction::PlayFrom(v, _) => ((v < n).then_some(Dest::Video(v)), Some(back)),
        SimAction::PlayAllVideos => (Some(Dest::PlayAll), Some(k)),
        SimAction::Prev => (k.checked_sub(1).and_then(page), None),
        SimAction::Next => (page(k + 1), None),
        SimAction::Go(t) => match t {
            Target::Video(v) => ((v < n).then_some(Dest::Video(v)), Some(back)),
            Target::Main(q) => (page(q), None),
            Target::Stay(e) | Target::Audio(_, e) => (e.and_then(page), None),
        },
    }
}

/// Pages ou l'on peut se trouver apres ce bouton (destination, ou retour apres la lecture).
fn next_pages(d: Option<Dest>, ret: Option<usize>) -> Vec<usize> {
    match d {
        Some(Dest::Page(q)) => vec![q],
        Some(Dest::Video(_) | Dest::PlayAll) => ret.into_iter().collect(),
        None => vec![],
    }
}

/// Analyse le projet. `duration(v)` : duree de la video `v` en secondes, si elle est connue (sinon les departs de chapitre ne
/// sont pas compares a la fin de la video).
pub fn check(m: &ProjectModel, duration: &dyn Fn(usize) -> Option<f64>) -> Report {
    let np = m.designs.free.len();
    let n = m.videos.len();
    let mut out: Vec<Finding> = Vec::new();
    if np == 0 {
        out.push(Finding { severity: Severity::Info, rule: Rule::Reach, place: Place::Page(0),
            msg: tr("No menu page: starter pages will be generated when the disc is built; nothing to check yet.").to_string() });
        let videos = (0..n).map(|v| VideoNode { title: video_title(m, v), from: vec![], returns: vec![] }).collect();
        return Report { findings: out, pages: vec![], videos, home: 0, generated: true };
    }
    let home = match m.designs.home { Some(Target::Main(q)) if q < np => q, _ => 0 };
    let style = &m.designs.style;
    let buttons: Vec<Vec<SimButton>> = (0..np).map(|k| sim_buttons(&PageInfo { r: PageRef(k), page: k, pages: np }, &m.designs.free[k], style, n)).collect();
    let effects: Vec<Vec<(Option<Dest>, Option<usize>)>> = buttons.iter().enumerate().map(|(k, bs)| bs.iter().map(|b| effect(b, k, np, home, n)).collect()).collect();
    let mut add = |severity: Severity, rule: Rule, place: Place, msg: String| out.push(Finding { severity, rule, place, msg });

    // 1. pages accessibles depuis l'accueil
    let mut reach = vec![false; np];
    let mut todo = vec![home];
    reach[home] = true;
    while let Some(k) = todo.pop() {
        for &(d, r) in &effects[k] {
            for q in next_pages(d, r) {
                if q < np && !reach[q] { reach[q] = true; todo.push(q); }
            }
        }
    }
    for q in (0..np).filter(|q| !reach[*q]) {
        add(Severity::Warning, Rule::Reach, Place::Page(q), trf!("Page « {} » cannot be reached: no button leads to it from the home page.", page_name(m, q)));
    }

    // 2. videos lancees (« Tout lire » lance toutes les videos)
    let mut from: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut returns: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, effs) in effects.iter().enumerate() {
        for &(d, r) in effs {
            let vs: Vec<usize> = match d { Some(Dest::Video(v)) => vec![v], Some(Dest::PlayAll) => (0..n).collect(), _ => vec![] };
            for v in vs {
                if !from[v].contains(&k) { from[v].push(k); }
                if let Some(r) = r { if !returns[v].contains(&r) { returns[v].push(r); } }
            }
        }
    }
    for v in 0..n {
        if from[v].is_empty() {
            add(Severity::Warning, Rule::Orphan, Place::Video(v), trf!("{}: no button plays it.", video_name(m, v)));
        } else if from[v].iter().all(|k| !reach[*k]) {
            let pages: Vec<String> = from[v].iter().map(|k| page_name(m, *k)).collect();
            add(Severity::Warning, Rule::Orphan, Place::Video(v), trf!("{}: only played from pages that cannot be reached ({}).", video_name(m, v), pages.join(", ")));
        }
    }

    // 3. retour vers une page supprimee
    for (k, bs) in buttons.iter().enumerate() {
        for b in bs {
            if let SlotReturn::Page(q) = b.ret {
                if q >= np && effects[k][bs.iter().position(|x| x.item == b.item).unwrap_or(0)].1.is_some() {
                    add(Severity::Warning, Rule::Return, Place::Item(k, b.item), trf!("{}: the return page no longer exists; playback returns to the thumbnail's page.", item_name(m, k, b.item)));
                }
            }
        }
    }

    // 4. boucles de lancement automatique : (page suivante, passe par une lecture, delai)
    let auto_next: Vec<Option<(usize, bool, u32)>> = (0..np).map(|k| {
        let idle = m.designs.free[k].idle?;
        let b = buttons[k].iter().position(|b| idle.item != 0 && m.designs.free[k].items[b.item].id == idle.item)?;
        let (d, r) = effects[k][b];
        let via_video = matches!(d, Some(Dest::Video(_) | Dest::PlayAll));
        next_pages(d, r).first().map(|q| (*q, via_video, idle.seconds))
    }).collect();
    let mut seen_cycles: Vec<Vec<usize>> = Vec::new();
    for start in 0..np {
        let mut path: Vec<usize> = Vec::new();
        let mut k = start;
        while let Some((q, _, _)) = auto_next[k] {
            if let Some(pos) = path.iter().position(|x| *x == k) {
                let cycle = path[pos..].to_vec();
                let mut key = cycle.clone();
                key.sort_unstable();
                if !seen_cycles.contains(&key) {
                    seen_cycles.push(key);
                    let video = cycle.iter().any(|c| auto_next[*c].is_some_and(|a| a.1));
                    let mut steps: Vec<String> = cycle.iter().map(|c| {
                        let (_, v, s) = auto_next[*c].unwrap();
                        if v { trf!("{} (auto {} s, plays a video)", page_name(m, *c), s) } else { trf!("{} (auto {} s)", page_name(m, *c), s) }
                    }).collect();
                    steps.push(page_name(m, cycle[0]));
                    let chain = steps.join(" → ");
                    if video {
                        add(Severity::Info, Rule::AutoLoop, Place::Page(cycle[0]), trf!("Endless playback without any key pressed: {} (demo loop?).", chain));
                    } else {
                        add(Severity::Warning, Rule::AutoLoop, Place::Page(cycle[0]), trf!("Loop without any key pressed: {}; the disc goes round in circles until a key is pressed.", chain));
                    }
                }
                break;
            }
            if path.len() > np { break; }
            path.push(k);
            k = q;
        }
    }

    // 5. navigation a la telecommande : tous les boutons joignables depuis celui qui est selectionne a l'ouverture
    let dirs = [tr("up"), tr("down"), tr("left"), tr("right")];
    for (k, bs) in buttons.iter().enumerate() {
        if bs.len() < 2 { continue; }
        let nb = |i: usize| -> [usize; 4] { let n = bs[i].neighbors; [n.0, n.1, n.2, n.3] };
        let mut seen = vec![false; bs.len()];
        let mut todo = vec![0usize];
        seen[0] = true;
        while let Some(i) = todo.pop() {
            for j in nb(i) {
                if j < bs.len() && !seen[j] { seen[j] = true; todo.push(j); }
            }
        }
        for (i, b) in bs.iter().enumerate().filter(|(i, _)| !seen[*i]) {
            add(Severity::Warning, Rule::Arrows, Place::Item(k, b.item), trf!("{} cannot be reached with the arrows from the button selected when the page opens ({}).", item_name(m, k, b.item), item_name(m, k, bs[0].item)));
            let _ = i;
        }
        // fleches choisies a la main sans retour
        for (i, b) in bs.iter().enumerate() {
            let nav = m.designs.free[k].items[b.item].nav;
            for d in 0..4 {
                if !matches!(nav.get(d), crate::design::NavLink::Item(_)) { continue; }
                let j = nb(i)[d];
                if j == i || j >= bs.len() { continue; }
                let back = nb(j)[[1, 0, 3, 2][d]];
                if back != i {
                    let other = if back == j { tr("does not move").to_string() } else { trf!("→ {}", item_name(m, k, bs[back].item)) };
                    add(Severity::Info, Rule::Arrows, Place::Item(k, b.item), trf!("One-way arrow: {} ▸ {} → {}, but {} ▸ {} {}.",
                        item_name(m, k, b.item), dirs[d], item_name(m, k, bs[j].item), item_name(m, k, bs[j].item), dirs[[1, 0, 3, 2][d]], other));
                }
            }
        }
    }

    // 6. chapitres
    for (k, d) in m.designs.free.iter().enumerate() {
        for (idx, it) in d.items.iter().enumerate() {
            if let crate::design::Kind::Slot { video: Some(v), chapter: Some(id), link: None, .. } = it.kind {
                if v < n && !m.videos[v].chapters.iter().any(|c| c.id == id) {
                    add(Severity::Info, Rule::Chapters, Place::Item(k, idx), trf!("{}: the linked chapter no longer exists in {}; the thumbnail keeps its time.", item_name(m, k, idx), video_name(m, v)));
                }
            }
        }
        for b in &buttons[k] {
            if let SimAction::PlayFrom(v, t) = b.action {
                if let Some(dur) = duration(v).filter(|d| *d > 0.0) {
                    if t >= dur {
                        add(Severity::Error, Rule::Chapters, Place::Item(k, b.item), trf!("{} starts at {}, after the end of {} ({}).", item_name(m, k, b.item), mmss(t), video_name(m, v), mmss(dur)));
                    }
                }
            }
        }
    }

    // 7. langues : reglage audio ou sous-titres sans piste dans certaines videos
    let has_audio = |v: usize, id: u32| -> bool {
        let ve = &m.videos[v];
        if ve.audio.is_empty() { m.language(id).is_some_and(|l| l.code == m.lang) } else { ve.audio.iter().any(|t| t.lang_id == id) }
    };
    let has_subs = |v: usize, id: u32| m.videos[v].subs.iter().any(|t| t.lang_id == id);
    for (k, bs) in buttons.iter().enumerate() {
        for (bi, b) in bs.iter().enumerate() {
            let played: Option<usize> = match effects[k][bi].0 { Some(Dest::Video(v)) => Some(v), _ => None };
            let concerned: Vec<usize> = played.map_or_else(|| (0..n).collect(), |v| vec![v]);
            if let SimAction::Go(Target::Audio(id, _)) = b.action {
                if m.language(id).is_some() {
                    let missing: Vec<usize> = concerned.iter().copied().filter(|v| !has_audio(*v, id)).collect();
                    // aucune video : deja signale par la validation du projet
                    if !missing.is_empty() && missing.len() < n {
                        add(Severity::Warning, Rule::Languages, Place::Item(k, b.item), trf!("{} (audio « {} »): {} without an audio track in this language.", item_name(m, k, b.item), m.language_name(id), videos_list(&missing)));
                    }
                }
            }
            if let SubChoice::Lang(id) = b.sub {
                if m.language(id).is_some() {
                    let missing: Vec<usize> = concerned.iter().copied().filter(|v| !has_subs(*v, id)).collect();
                    if !missing.is_empty() {
                        add(Severity::Warning, Rule::Languages, Place::Item(k, b.item), trf!("{} (subtitles « {} »): {} without subtitles in this language.", item_name(m, k, b.item), m.language_name(id), videos_list(&missing)));
                    }
                }
            }
        }
    }

    // 8. boutons qui font exactement la meme chose sur une page
    for (k, bs) in buttons.iter().enumerate() {
        let key = |bi: usize| -> String {
            let b = &bs[bi];
            let (d, r) = effects[k][bi];
            let act = match b.action {
                SimAction::PlayFrom(v, t) => format!("v{v}@{t:.2}|{r:?}"),
                SimAction::Go(Target::Stay(e)) => format!("s{e:?}"),
                SimAction::Go(Target::Audio(id, e)) => format!("a{id}|{e:?}"),
                _ => format!("{d:?}|{r:?}"),
            };
            format!("{act}|{:?}", b.sub)
        };
        let keys: Vec<String> = (0..bs.len()).map(key).collect();
        let mut done = vec![false; bs.len()];
        for i in 0..bs.len() {
            if done[i] || effects[k][i].0.is_none() && !matches!(bs[i].action, SimAction::Go(Target::Stay(_) | Target::Audio(..))) { continue; }
            let same: Vec<usize> = (i + 1..bs.len()).filter(|j| keys[*j] == keys[i]).collect();
            if same.is_empty() { continue; }
            for j in &same { done[*j] = true; }
            let mut names: Vec<String> = std::iter::once(i).chain(same).map(|j| item_name(m, k, bs[j].item)).collect();
            let last = names.pop().unwrap_or_default();
            add(Severity::Info, Rule::Duplicates, Place::Item(k, bs[i].item), trf!("{} and {} do exactly the same thing.", names.join(", "), last));
        }
    }

    // carte du disque
    let pages = (0..np).map(|k| {
        let idle = m.designs.free[k].idle;
        let mut exits: Vec<Exit> = Vec::new();
        for (bi, b) in buttons[k].iter().enumerate() {
            let Some(d) = effects[k][bi].0 else { continue };
            let auto = idle.filter(|i| i.item != 0 && m.designs.free[k].items[b.item].id == i.item).map(|i| i.seconds);
            match exits.iter_mut().find(|e| e.dest == d) {
                Some(e) => { if auto.is_some() { e.auto_s = auto; } }
                None => exits.push(Exit { dest: d, auto_s: auto }),
            }
        }
        PageNode { name: page_name(m, k), reachable: reach[k], exits }
    }).collect();
    let videos = (0..n).map(|v| VideoNode { title: video_title(m, v), from: from[v].clone(), returns: returns[v].clone() }).collect();
    out.sort_by_key(|f| f.severity);
    Report { findings: out, pages, videos, home, generated: false }
}
