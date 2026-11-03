//! Verificateur de dossier BDMV : references croisees entre fichiers, sans lecteur (equivalent de `check.py`).
//!
//! Controles : index -> objets ; commandes (playlists, titres, objets, sauts) a operandes immediats ;
//! playlists -> clips (.clpi, .m2ts), bornes de temps, flux declares ; .clpi -> taille du .m2ts, EP_map ;
//! .m2ts -> structure ; menus IG (commandes, voisins) ; copies BACKUP identiques.
use crate::clpi::Clpi;
use crate::i18n::tr;
use crate::trf;
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
            let t = match k { Kind::Error => tr("ERROR"), Kind::Warn => tr("warning"), Kind::Info => tr("ok") };
            s += &format!("[{t}] {m}\n");
        }
        s + &trf!("RESULT: {}\n", if self.ok() { tr("no errors") } else { tr("errors detected") })
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
                        r.err(trf!("{} command {} {} {}: {}.mpls missing", where_, j, name, c.dst, format!("{:05}", c.dst)));
                    }
                } else {
                    r.warn(trf!("{} command {} {}: indirect playlist (register), not followed", where_, j, name));
                }
            }
            "JUMP_TITLE" | "CALL_TITLE" if imm && c.dst as usize > n_titles && c.dst != 0xFFFF => {
                r.err(trf!("{} command {} {} {}: only {} title(s)", where_, j, name, c.dst, n_titles));
            }
            "JUMP_OBJECT" | "CALL_OBJECT" if imm && c.dst as usize >= n_obj => {
                r.err(trf!("{} command {} {} {}: object does not exist", where_, j, name, c.dst));
            }
            "GOTO" if imm && c.dst as usize >= cmds.len() => {
                r.err(trf!("{} command {} GOTO {}: outside the object", where_, j, c.dst));
            }
            _ => {}
        }
    }
}

pub fn check_bdmv(root: &Path) -> Report {
    let mut r = Report::default();
    let (Some(idx_b), Some(mo_b)) = (read(root, "index.bdmv"), read(root, "MovieObject.bdmv")) else {
        r.err(tr("index.bdmv or MovieObject.bdmv missing"));
        return r;
    };
    let (index, mobj) = match (IndexBdmv::parse(&idx_b), MovieObjectBdmv::parse(&mo_b)) {
        (Ok(i), Ok(m)) => (i, m),
        (Err(e), _) => { r.err(trf!("index.bdmv unreadable: {}", e)); return r }
        (_, Err(e)) => { r.err(trf!("MovieObject.bdmv unreadable: {}", e)); return r }
    };
    let (n_obj, n_titles) = (mobj.objects.len(), index.titles.len());
    let mut playlists: Vec<String> = std::fs::read_dir(root.join("PLAYLIST"))
        .map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".mpls").map(String::from))).collect())
        .unwrap_or_default();
    playlists.sort();
    r.info(trf!("{} title(s), {} object(s), {} playlist(s)", n_titles, n_obj, playlists.len()));

    for (name, o) in [("First Playback", &index.first_playback), ("Top Menu", &index.top_menu)] {
        if o.id_ref as usize >= n_obj { r.err(trf!("{} -> object {} does not exist", name, o.id_ref)); }
    }
    for (i, t) in index.titles.iter().enumerate() {
        if t.id_ref as usize >= n_obj { r.err(trf!("title {} -> object {} does not exist", i + 1, t.id_ref)); }
    }
    for (i, o) in mobj.objects.iter().enumerate() {
        check_cmds(&mut r, &trf!("object {}", i), &o.commands, &playlists, n_titles, n_obj);
    }

    // clips (charges a la demande, dans l'ordre de premiere reference)
    let mut clips: BTreeMap<String, Option<ClipData>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    fn load<'a>(r: &mut Report, root: &Path, clips: &'a mut BTreeMap<String, Option<ClipData>>, order: &mut Vec<String>, num: &str) -> Option<&'a ClipData> {
        if !clips.contains_key(num) {
            let m2ts = root.join(format!("STREAM/{num}.m2ts"));
            let v = match read(root, &format!("CLIPINF/{num}.clpi")) {
                None => { r.err(trf!("clip {}: {}.clpi missing", num, num)); None }
                Some(_) if !m2ts.is_file() => { r.err(trf!("clip {}: {}.m2ts missing", num, num)); None }
                Some(b) => match Clpi::parse(&b) {
                    Ok(clpi) => Some(ClipData { clpi, size: std::fs::metadata(&m2ts).map(|m| m.len()).unwrap_or(0), path: m2ts }),
                    Err(e) => { r.err(trf!("clip {}: .clpi unreadable ({})", num, e)); None }
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
            Some(Err(e)) => { r.err(trf!("playlist {} unreadable: {}", pl, e)); continue }
            None => continue,
        };
        for (k, it) in m.playitems.iter().enumerate() {
            let Some(c) = load(&mut r, root, &mut clips, &mut order, &it.clip) else { continue };
            let Some(seq) = c.clpi.sequence.atc.first().and_then(|a| a.stc.first()).cloned() else { continue };
            let streams = c.clpi.program.programs.first().map(|p| p.streams.clone()).unwrap_or_default();
            let (clip_name, ok_time) = (it.clip.clone(), seq.start_time <= it.in_time && it.in_time < it.out_time && it.out_time <= seq.end_time);
            if !ok_time {
                r.err(trf!("playlist {} item {}: time {}-{} outside the sequence of clip {} ({}-{})", pl, k, it.in_time, it.out_time, clip_name, seq.start_time, seq.end_time));
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
                    r.err(trf!("playlist {} item {}: stream PID {} type {} missing from clip {}", pl, k, format!("{:#x}", pid), format!("{:#x}", coding), clip_name));
                }
                pos += 2 + ln + alen;
            }
        }
        for sp in &m.subpaths {
            for si in &sp.items {
                let Some(c) = load(&mut r, root, &mut clips, &mut order, &si.clip) else { continue };
                if let Some(seq) = c.clpi.sequence.atc.first().and_then(|a| a.stc.first()) {
                    if !(seq.start_time <= si.in_time && si.in_time < si.out_time && si.out_time <= seq.end_time) {
                        let msg = trf!("playlist {} sub-path: time {}-{} outside the sequence of clip {} ({}-{})", pl, si.in_time, si.out_time, si.clip, seq.start_time, seq.end_time);
                        r.err(msg);
                    }
                }
            }
        }
        if m.marks.is_empty() {
            r.warn(trf!("playlist {}: no chapter mark", pl));
        } else {
            // chaque marque se rapporte a son PlayItem (playlists « Tout lire » : plusieurs elements)
            let mut last: Option<(u16, u32)> = None;
            for (k, mk) in m.marks.iter().enumerate() {
                let Some(it) = m.playitems.get(mk.playitem_ref as usize) else {
                    r.err(trf!("playlist {} mark {}: item {} does not exist", pl, k, mk.playitem_ref));
                    continue;
                };
                if let Some((ref_, t)) = last {
                    if mk.playitem_ref < ref_ || (mk.playitem_ref == ref_ && mk.time < t) {
                        r.err(trf!("playlist {}: chapter marks not increasing", pl));
                    }
                }
                last = Some((mk.playitem_ref, mk.time));
                let eps: Vec<u64> = match clips.get(&it.clip) {
                    Some(Some(c)) => c.clpi.cpi.iter().flat_map(|cp| cp.streams.iter()).flat_map(|s| s.entries()).map(|e| e.0).collect(),
                    _ => continue,
                };
                if !(it.in_time <= mk.time && mk.time <= it.out_time) {
                    r.err(trf!("playlist {} mark {}: time {} outside the PlayItem ({}-{})", pl, k, mk.time, it.in_time, it.out_time));
                } else if !eps.is_empty() && mk.time != it.in_time && !eps.iter().any(|&e| { let d = mk.time as i64 * 2 - e as i64; (0..1024).contains(&d) }) {
                    r.warn(trf!("playlist {} mark {}: not on an EP_map access picture", pl, k));
                }
            }
        }
    }

    // clips -> structure
    for num in &order {
        let Some(Some(c)) = clips.get(num) else { continue };
        let clpi = &c.clpi;
        if c.size != clpi.clip_info.num_source_packets as u64 * 192 {
            r.err(trf!("clip {}: {} bytes but the .clpi announces {} packets", num, c.size, clpi.clip_info.num_source_packets));
        }
        if c.size % 6144 != 0 { r.warn(trf!("clip {}: size not a multiple of 6144 bytes", num)); }
        let data = match std::fs::read(&c.path) { Ok(d) => d, Err(e) => { r.err(trf!("clip {}: read failed ({})", num, e)); continue } };
        let mut pids = BTreeSet::new();
        match iter_packets(&data) {
            Ok(it) => for p in it { match p { Ok(p) => { pids.insert(p.pid); } Err(e) => { r.err(trf!("clip {}: {}", num, e)); break } } },
            Err(e) => { r.err(trf!("clip {}: {}", num, e)); continue }
        }
        let prog = clpi.program.programs.first();
        let declared: BTreeSet<u16> = prog.map(|p| p.streams.iter().map(|s| s.pid).collect()).unwrap_or_default();
        let pcr = clpi.sequence.atc.first().and_then(|a| a.stc.first()).map(|s| s.pcr_pid).unwrap_or(0);
        if !(pids.contains(&0) && prog.map_or(false, |p| pids.contains(&p.pmt_pid)) && pids.contains(&pcr)) {
            r.err(trf!("clip {}: PAT, PMT or PCR missing from the stream", num));
        }
        if !declared.is_subset(&pids) {
            let miss: Vec<String> = declared.difference(&pids).map(|p| format!("{p:#x}")).collect();
            r.err(trf!("clip {}: declared streams {} missing from the .m2ts", num, format!("{:?}", miss)));
        }
        if let Some(cpi) = &clpi.cpi {
            for st in &cpi.streams {
                let spns: Vec<u32> = st.entries().iter().map(|e| e.1).collect();
                if spns.windows(2).any(|w| w[0] > w[1]) || spns.last().map_or(false, |&l| l >= clpi.clip_info.num_source_packets) {
                    r.err(trf!("clip {}: inconsistent EP_map (non-increasing SPNs or outside the file)", num));
                }
            }
        }
        r.info(trf!("clip {}: {} packets, {} streams, EP_map {}", num, c.size / 192, declared.len(), if clpi.cpi.is_some() { tr("yes") } else { tr("no") }));
    }

    // menus IG
    for num in order.clone() {
        let Some(Some(c)) = clips.get(&num) else { continue };
        let Some(ig_pid) = c.clpi.program.programs.first().and_then(|p| p.streams.iter().find(|s| s.coding_type == 0x91)).map(|s| s.pid) else { continue };
        let data = std::fs::read(&c.path).unwrap_or_default();
        let mut segs = Vec::new();
        match reassemble(&data, ig_pid) {
            Ok(pes) => for e in pes { match split_segments(&e.data) { Ok(s) => segs.extend(s), Err(e) => r.err(trf!("menu {}: {}", num, e)) } },
            Err(e) => { r.err(trf!("menu {}: {}", num, e)); continue }
        }
        let Some(ics_seg) = segs.iter().find(|s| s.name() == "ICS") else { r.err(trf!("clip {}: IG stream without ICS", num)); continue };
        let ics = match Ics::parse(&ics_seg.payload) { Ok(i) => i, Err(e) => { r.err(trf!("menu {}: ICS unreadable ({})", num, e)); continue } };
        let mut n_btn = 0;
        for page in &ics.pages {
            let ids: BTreeSet<u16> = page.bogs.iter().flat_map(|g| g.buttons.iter().map(|b| b.button_id)).collect();
            for g in &page.bogs {
                for b in &g.buttons {
                    n_btn += 1;
                    check_cmds(&mut r, &trf!("menu {} button {}", num, b.button_id), &b.commands, &playlists, n_titles, n_obj);
                    for nb in [b.upper, b.lower, b.left, b.right] {
                        if !ids.contains(&nb) { r.err(trf!("menu {} button {}: neighbor {} does not exist", num, b.button_id, nb)); }
                    }
                }
            }
            if !ids.contains(&page.default_selected) && page.default_selected != 0xFFFF {
                r.err(trf!("menu {}: default selected button {} does not exist", num, page.default_selected));
            }
        }
        r.info(trf!("menu {}: {} button(s), commands checked", num, n_btn));
    }

    // sous-titres PGS
    for num in order.clone() {
        let Some(Some(c)) = clips.get(&num) else { continue };
        let pgs: Vec<u16> = c.clpi.program.programs.first().map(|p| p.streams.iter().filter(|s| s.coding_type == 0x90).map(|s| s.pid).collect()).unwrap_or_default();
        if pgs.is_empty() { continue }
        let data = std::fs::read(&c.path).unwrap_or_default();
        for pid in pgs {
            let pes = match reassemble(&data, pid) {
                Ok(p) => p,
                Err(e) => { r.err(trf!("subtitles {} PID {}: {}", num, format!("{:#x}", pid), e)); continue }
            };
            let (mut sets, mut shown, mut last_pts, mut open, mut bad) = (0usize, 0usize, 0u64, false, false);
            for e in &pes {
                let segs = match crate::pgs::parse_segments(&e.data) {
                    Ok(s) => s,
                    Err(m) => { r.err(trf!("subtitles {} PID {}: {}", num, format!("{:#x}", pid), m)); bad = true; break }
                };
                for (t, p) in segs {
                    match t {
                        crate::pgs::SEG_PCS => {
                            if open { r.err(trf!("subtitles {} PID {}: display set without end", num, format!("{:#x}", pid))); bad = true; }
                            open = true;
                            let pts = e.pts.unwrap_or(0);
                            if pts < last_pts { r.err(trf!("subtitles {} PID {}: non-increasing PTS", num, format!("{:#x}", pid))); bad = true; }
                            last_pts = pts;
                            if p.len() > 10 && p[10] > 0 { shown += 1; }
                            if e.dts.is_none() { r.warn(trf!("subtitles {} PID {}: PCS without DTS", num, format!("{:#x}", pid))); }
                        }
                        crate::pgs::SEG_END => { if !open { r.err(trf!("subtitles {} PID {}: end without composition", num, format!("{:#x}", pid))); bad = true; } open = false; sets += 1; }
                        crate::pgs::SEG_WDS | crate::pgs::SEG_PDS | crate::pgs::SEG_ODS => { if !open { r.err(trf!("subtitles {} PID {}: segment outside composition", num, format!("{:#x}", pid))); bad = true; } }
                        t => { r.err(trf!("subtitles {} PID {}: unexpected segment {}", num, format!("{:#x}", pid), format!("{:#x}", t))); bad = true; }
                    }
                }
                if bad { break }
            }
            if open && !bad { r.err(trf!("subtitles {} PID {}: last set without end", num, format!("{:#x}", pid))); bad = true; }
            if !bad { r.info(trf!("subtitles {} PID {}: {} display set(s), {} subtitle(s) shown", num, format!("{:#x}", pid), sets, shown)); }
        }
    }

    // BACKUP
    for rel in ["index.bdmv", "MovieObject.bdmv"] {
        if read(root, rel) != read(root, &format!("BACKUP/{rel}")) { r.warn(trf!("BACKUP/{} missing or different", rel)); }
    }
    for (sub, ext) in [("PLAYLIST", ".mpls"), ("CLIPINF", ".clpi")] {
        let mut files: Vec<String> = std::fs::read_dir(root.join(sub)).map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
        files.sort();
        for f in files.iter().filter(|f| f.ends_with(ext)) {
            if read(root, &format!("{sub}/{f}")) != read(root, &format!("BACKUP/{sub}/{f}")) { r.warn(trf!("BACKUP/{}/{} missing or different", sub, f)); }
        }
    }
    r
}
