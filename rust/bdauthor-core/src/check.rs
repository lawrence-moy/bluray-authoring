//! Verificateur de dossier BDMV : references croisees entre fichiers, sans lecteur (equivalent de `check.py`).
//!
//! Controles : index -> objets ; commandes (playlists, titres, objets, sauts) a operandes immediats ;
//! playlists -> clips (.clpi, .m2ts), bornes de temps, flux declares ; .clpi -> taille du .m2ts, EP_map ;
//! .m2ts -> structure ; menus IG (commandes, voisins) ; copies BACKUP identiques.
use crate::clpi::Clpi;
use crate::ig::Ics;
use crate::index::IndexBdmv;
use crate::m2ts::{iter_packets, reassemble, split_segments};
use crate::mobj::{Command, MovieObjectBdmv};
use crate::mpls::Mpls;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind { Error, Warn, Info }

#[derive(Debug, Default)]
pub struct Report { pub items: Vec<(Kind, String)> }

impl Report {
    pub fn err(&mut self, m: impl Into<String>) { self.items.push((Kind::Error, m.into())) }
    pub fn warn(&mut self, m: impl Into<String>) { self.items.push((Kind::Warn, m.into())) }
    pub fn info(&mut self, m: impl Into<String>) { self.items.push((Kind::Info, m.into())) }
    pub fn ok(&self) -> bool { !self.items.iter().any(|(k, _)| *k == Kind::Error) }
    pub fn render(&self) -> String {
        let mut s = String::new();
        for (k, m) in &self.items {
            let t = match k { Kind::Error => "ERREUR", Kind::Warn => "avertissement", Kind::Info => "ok" };
            s += &format!("[{t}] {m}\n");
        }
        s + &format!("RESULTAT : {}\n", if self.ok() { "aucune erreur" } else { "erreurs detectees" })
    }
}

fn read(root: &Path, rel: &str) -> Option<Vec<u8>> {
    std::fs::read(root.join(rel)).ok()
}

struct ClipData { clpi: Clpi, size: u64, path: PathBuf }

fn check_cmds(r: &mut Report, where_: &str, cmds: &[Command], playlists: &[String], n_titles: usize, n_obj: usize) {
    for (j, c) in cmds.iter().enumerate() {
        let text = c.text();
        let name = text.split_whitespace().next().unwrap_or("");
        let imm = c.imm_op1();
        match name {
            "PLAY_PL" | "PLAY_PL_PI" | "PLAY_PL_PM" => {
                if imm {
                    if !playlists.contains(&format!("{:05}", c.dst)) {
                        r.err(format!("{where_} commande {j} {name} {} : {:05}.mpls absente", c.dst, c.dst));
                    }
                } else {
                    r.warn(format!("{where_} commande {j} {name} : playlist indirecte (registre), non suivie"));
                }
            }
            "JUMP_TITLE" | "CALL_TITLE" if imm && c.dst as usize > n_titles && c.dst != 0xFFFF => {
                r.err(format!("{where_} commande {j} {name} {} : seulement {n_titles} titre(s)", c.dst));
            }
            "JUMP_OBJECT" | "CALL_OBJECT" if imm && c.dst as usize >= n_obj => {
                r.err(format!("{where_} commande {j} {name} {} : objet inexistant", c.dst));
            }
            "GOTO" if imm && c.dst as usize >= cmds.len() => {
                r.err(format!("{where_} commande {j} GOTO {} : hors de l'objet", c.dst));
            }
            _ => {}
        }
    }
}

pub fn check_bdmv(root: &Path) -> Report {
    let mut r = Report::default();
    let (Some(idx_b), Some(mo_b)) = (read(root, "index.bdmv"), read(root, "MovieObject.bdmv")) else {
        r.err("index.bdmv ou MovieObject.bdmv absent");
        return r;
    };
    let (index, mobj) = match (IndexBdmv::parse(&idx_b), MovieObjectBdmv::parse(&mo_b)) {
        (Ok(i), Ok(m)) => (i, m),
        (Err(e), _) => { r.err(format!("index.bdmv illisible : {e}")); return r }
        (_, Err(e)) => { r.err(format!("MovieObject.bdmv illisible : {e}")); return r }
    };
    let (n_obj, n_titles) = (mobj.objects.len(), index.titles.len());
    let mut playlists: Vec<String> = std::fs::read_dir(root.join("PLAYLIST"))
        .map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".mpls").map(String::from))).collect())
        .unwrap_or_default();
    playlists.sort();
    r.info(format!("{n_titles} titre(s), {n_obj} objet(s), {} playlist(s)", playlists.len()));

    for (name, o) in [("First Playback", &index.first_playback), ("Top Menu", &index.top_menu)] {
        if o.id_ref as usize >= n_obj { r.err(format!("{name} -> objet {} inexistant", o.id_ref)); }
    }
    for (i, t) in index.titles.iter().enumerate() {
        if t.id_ref as usize >= n_obj { r.err(format!("titre {} -> objet {} inexistant", i + 1, t.id_ref)); }
    }
    for (i, o) in mobj.objects.iter().enumerate() {
        check_cmds(&mut r, &format!("objet {i}"), &o.commands, &playlists, n_titles, n_obj);
    }

    // clips (charges a la demande, dans l'ordre de premiere reference)
    let mut clips: BTreeMap<String, Option<ClipData>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    fn load<'a>(r: &mut Report, root: &Path, clips: &'a mut BTreeMap<String, Option<ClipData>>, order: &mut Vec<String>, num: &str) -> Option<&'a ClipData> {
        if !clips.contains_key(num) {
            let m2ts = root.join(format!("STREAM/{num}.m2ts"));
            let v = match read(root, &format!("CLIPINF/{num}.clpi")) {
                None => { r.err(format!("clip {num} : {num}.clpi absent")); None }
                Some(_) if !m2ts.is_file() => { r.err(format!("clip {num} : {num}.m2ts absent")); None }
                Some(b) => match Clpi::parse(&b) {
                    Ok(clpi) => Some(ClipData { clpi, size: std::fs::metadata(&m2ts).map(|m| m.len()).unwrap_or(0), path: m2ts }),
                    Err(e) => { r.err(format!("clip {num} : .clpi illisible ({e})")); None }
                },
            };
            order.push(num.to_string());
            clips.insert(num.to_string(), v);
        }
        clips[num].as_ref()
    }

    for pl in &playlists {
        let m = match read(root, &format!("PLAYLIST/{pl}.mpls")).map(|b| Mpls::parse(&b)) {
            Some(Ok(m)) => m,
            Some(Err(e)) => { r.err(format!("playlist {pl} illisible : {e}")); continue }
            None => continue,
        };
        for (k, it) in m.playitems.iter().enumerate() {
            let Some(c) = load(&mut r, root, &mut clips, &mut order, &it.clip) else { continue };
            let Some(seq) = c.clpi.sequence.atc.first().and_then(|a| a.stc.first()).cloned() else { continue };
            let streams = c.clpi.program.programs.first().map(|p| p.streams.clone()).unwrap_or_default();
            let (clip_name, ok_time) = (it.clip.clone(), seq.start_time <= it.in_time && it.in_time < it.out_time && it.out_time <= seq.end_time);
            if !ok_time {
                r.err(format!("playlist {pl} item {k} : temps {}-{} hors de la sequence du clip {clip_name} ({}-{})", it.in_time, it.out_time, seq.start_time, seq.end_time));
            }
            let pids: BTreeMap<u16, u8> = streams.iter().map(|s| (s.pid, s.coding_type)).collect();
            let start = 12 + 2 + 2 + 12;
            let stn = if it.tail.len() > start { &it.tail[start..] } else { &[][..] };
            let mut pos = 0usize;
            while pos + 2 < stn.len() {
                let ln = stn[pos] as usize;
                if ln == 0 { break; }
                let typ = stn[pos + 1];
                let at = if typ == 1 { pos + 2 } else { pos + 4 };
                if pos + 2 + ln >= stn.len() || at + 2 > stn.len() { break; }
                let pid = u16::from_be_bytes([stn[at], stn[at + 1]]);
                let alen = stn[pos + 1 + ln] as usize;
                let coding = stn[pos + 2 + ln];
                if typ == 1 && pids.get(&pid) != Some(&coding) {
                    r.err(format!("playlist {pl} item {k} : flux PID {pid:#x} type {coding:#x} absent du clip {clip_name}"));
                }
                pos += 2 + ln + alen;
            }
        }
        for sp in &m.subpaths {
            for si in &sp.items {
                let Some(c) = load(&mut r, root, &mut clips, &mut order, &si.clip) else { continue };
                if let Some(seq) = c.clpi.sequence.atc.first().and_then(|a| a.stc.first()) {
                    if !(seq.start_time <= si.in_time && si.in_time < si.out_time && si.out_time <= seq.end_time) {
                        let msg = format!("playlist {pl} sous-chemin : temps {}-{} hors de la sequence du clip {} ({}-{})", si.in_time, si.out_time, si.clip, seq.start_time, seq.end_time);
                        r.err(msg);
                    }
                }
            }
        }
        if m.marks.is_empty() {
            r.warn(format!("playlist {pl} : aucune marque de chapitre"));
        } else if let Some(it0) = m.playitems.first() {
            let eps: Vec<u64> = match clips.get(&it0.clip) {
                Some(Some(c)) => c.clpi.cpi.iter().flat_map(|cp| cp.streams.iter()).flat_map(|s| s.entries()).map(|e| e.0).collect(),
                _ => continue,
            };
            let times: Vec<u32> = m.marks.iter().map(|k| k.time).collect();
            if times.windows(2).any(|w| w[0] > w[1]) {
                r.err(format!("playlist {pl} : marques de chapitre non croissantes"));
            }
            for (k, mk) in m.marks.iter().enumerate() {
                if !(it0.in_time <= mk.time && mk.time <= it0.out_time) {
                    r.err(format!("playlist {pl} marque {k} : temps {} hors du PlayItem ({}-{})", mk.time, it0.in_time, it0.out_time));
                } else if !eps.is_empty() && mk.time != it0.in_time && !eps.iter().any(|&e| { let d = mk.time as i64 * 2 - e as i64; (0..1024).contains(&d) }) {
                    r.warn(format!("playlist {pl} marque {k} : pas sur une image d'acces de l'EP_map"));
                }
            }
        }
    }

    // clips -> structure
    for num in &order {
        let Some(Some(c)) = clips.get(num) else { continue };
        let clpi = &c.clpi;
        if c.size != clpi.clip_info.num_source_packets as u64 * 192 {
            r.err(format!("clip {num} : {} octets mais le .clpi annonce {} paquets", c.size, clpi.clip_info.num_source_packets));
        }
        if c.size % 6144 != 0 { r.warn(format!("clip {num} : taille non multiple de 6144 octets")); }
        let data = match std::fs::read(&c.path) { Ok(d) => d, Err(e) => { r.err(format!("clip {num} : lecture impossible ({e})")); continue } };
        let mut pids = BTreeSet::new();
        match iter_packets(&data) {
            Ok(it) => for p in it { match p { Ok(p) => { pids.insert(p.pid); } Err(e) => { r.err(format!("clip {num} : {e}")); break } } },
            Err(e) => { r.err(format!("clip {num} : {e}")); continue }
        }
        let prog = clpi.program.programs.first();
        let declared: BTreeSet<u16> = prog.map(|p| p.streams.iter().map(|s| s.pid).collect()).unwrap_or_default();
        let pcr = clpi.sequence.atc.first().and_then(|a| a.stc.first()).map(|s| s.pcr_pid).unwrap_or(0);
        if !(pids.contains(&0) && prog.map_or(false, |p| pids.contains(&p.pmt_pid)) && pids.contains(&pcr)) {
            r.err(format!("clip {num} : PAT, PMT ou PCR absents du flux"));
        }
        if !declared.is_subset(&pids) {
            let miss: Vec<String> = declared.difference(&pids).map(|p| format!("{p:#x}")).collect();
            r.err(format!("clip {num} : flux declares {miss:?} absents du .m2ts"));
        }
        if let Some(cpi) = &clpi.cpi {
            for st in &cpi.streams {
                let spns: Vec<u32> = st.entries().iter().map(|e| e.1).collect();
                if spns.windows(2).any(|w| w[0] > w[1]) || spns.last().map_or(false, |&l| l >= clpi.clip_info.num_source_packets) {
                    r.err(format!("clip {num} : EP_map incoherente (SPN non croissants ou hors du fichier)"));
                }
            }
        }
        r.info(format!("clip {num} : {} paquets, {} flux, EP_map {}", c.size / 192, declared.len(), if clpi.cpi.is_some() { "oui" } else { "non" }));
    }

    // menus IG
    for num in order.clone() {
        let Some(Some(c)) = clips.get(&num) else { continue };
        let Some(ig_pid) = c.clpi.program.programs.first().and_then(|p| p.streams.iter().find(|s| s.coding_type == 0x91)).map(|s| s.pid) else { continue };
        let data = std::fs::read(&c.path).unwrap_or_default();
        let mut segs = Vec::new();
        match reassemble(&data, ig_pid) {
            Ok(pes) => for e in pes { match split_segments(&e.data) { Ok(s) => segs.extend(s), Err(e) => r.err(format!("menu {num} : {e}")) } },
            Err(e) => { r.err(format!("menu {num} : {e}")); continue }
        }
        let Some(ics_seg) = segs.iter().find(|s| s.name() == "ICS") else { r.err(format!("clip {num} : flux IG sans ICS")); continue };
        let ics = match Ics::parse(&ics_seg.payload) { Ok(i) => i, Err(e) => { r.err(format!("menu {num} : ICS illisible ({e})")); continue } };
        let mut n_btn = 0;
        for page in &ics.pages {
            let ids: BTreeSet<u16> = page.bogs.iter().flat_map(|g| g.buttons.iter().map(|b| b.button_id)).collect();
            for g in &page.bogs {
                for b in &g.buttons {
                    n_btn += 1;
                    check_cmds(&mut r, &format!("menu {num} bouton {}", b.button_id), &b.commands, &playlists, n_titles, n_obj);
                    for nb in [b.upper, b.lower, b.left, b.right] {
                        if !ids.contains(&nb) { r.err(format!("menu {num} bouton {} : voisin {nb} inexistant", b.button_id)); }
                    }
                }
            }
            if !ids.contains(&page.default_selected) && page.default_selected != 0xFFFF {
                r.err(format!("menu {num} : bouton selectionne par defaut {} inexistant", page.default_selected));
            }
        }
        r.info(format!("menu {num} : {n_btn} bouton(s), commandes verifiees"));
    }

    // BACKUP
    for rel in ["index.bdmv", "MovieObject.bdmv"] {
        if read(root, rel) != read(root, &format!("BACKUP/{rel}")) { r.warn(format!("BACKUP/{rel} absent ou different")); }
    }
    for (sub, ext) in [("PLAYLIST", ".mpls"), ("CLIPINF", ".clpi")] {
        let mut files: Vec<String> = std::fs::read_dir(root.join(sub)).map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
        files.sort();
        for f in files.iter().filter(|f| f.ends_with(ext)) {
            if read(root, &format!("{sub}/{f}")) != read(root, &format!("BACKUP/{sub}/{f}")) { r.warn(format!("BACKUP/{sub}/{f} absent ou different")); }
        }
    }
    r
}
