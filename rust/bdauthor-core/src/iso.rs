//! Image ISO du disque : systeme de fichiers UDF 2.50 (norme Blu-ray), avec partition de metadonnees et sa copie miroir.
//!
//! Disposition (secteurs de 2048 octets) :
//! - 0..16 : zone systeme (vide) ; 16..19 : sequence de reconnaissance BEA01 / NSR03 / TEA01 ;
//! - 32.. : sequence principale des descripteurs de volume (PVD, IUVD, PD, LVD, USD, TD) ; 48.. : sequence de reserve ;
//! - 64 : descripteur d'integrite du volume logique (LVID) + TD ; 256 : ancre (AVDP), repetee sur le dernier secteur ;
//! - partition physique a partir de `PART_START` : entrees du fichier de metadonnees et de son miroir, puis le contenu des
//!   metadonnees (descripteur d'ensemble de fichiers, entrees de fichiers et repertoires), les donnees des fichiers, et la
//!   copie miroir des metadonnees.
//!
//! Les entrees de fichiers sont dans la partition de metadonnees (reference 1) ; les donnees des fichiers dans la partition
//! physique (reference 0, descripteurs longs). Tout est en ecriture sequentielle : les fichiers sont recopies tels quels.
use crate::i18n::tr;
use crate::trf;
use crate::ffmpeg::{Error, Result};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const SECTOR: u64 = 2048;
const BS: usize = 2048;
/// Debut de la partition physique (multiple de 32 secteurs : blocs de 64 Ko alignes sur le disque).
const PART_START: u32 = 288;
/// Unite d'allocation et d'alignement (blocs) : 32 x 2 Ko = 64 Ko, taille d'un bloc de correction d'erreurs du Blu-ray.
const ALIGN: u32 = 32;
/// Longueur maximale d'une etendue (< 2^30, multiple de 2048).
pub const MAX_EXTENT: u32 = 0x3FFF_F800;
/// Capacites des disques (octets) : BD-25, BD-50, BD-100.
pub const CAPACITIES: [(&str, u64); 3] = [("BD-25", 25_025_314_816), ("BD-50", 50_050_629_632), ("BD-100", 100_103_356_416)];
/// Longueur maximale du nom du volume (caracteres latins ; moitie pour les autres ecritures).
pub const MAX_LABEL: usize = 30;

/// Ce qui sera garde d'un nom de volume (30 caracteres latins, ou 15 si le nom contient d'autres caracteres).
pub fn fit_label(s: &str) -> String { cs0_decode(&cs0(s, 31)) }

/// Ce qui sera garde d'une information du volume (proprietaire, organisation, contact : 34 ou 17 caracteres).
pub fn fit_info(s: &str) -> String { cs0_decode(&cs0(s, 35)) }

/// Date et heure choisies (temps universel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoStamp { pub year: u16, pub month: u8, pub day: u8, pub hour: u8, pub minute: u8 }

impl IsoStamp {
    pub fn days_in_month(year: u16, month: u8) -> u8 {
        match month { 4 | 6 | 9 | 11 => 30, 2 => if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 { 29 } else { 28 }, _ => 31 }
    }
    /// Ramene chaque champ dans sa plage (annee 1900 a 2100).
    pub fn clamped(self) -> IsoStamp {
        let year = self.year.clamp(1900, 2100);
        let month = self.month.clamp(1, 12);
        IsoStamp { year, month, day: self.day.clamp(1, Self::days_in_month(year, month)), hour: self.hour.min(23), minute: self.minute.min(59) }
    }
    /// « AAAA-MM-JJ HH:MM » (l'heure est facultative a la lecture).
    pub fn parse(t: &str) -> Option<IsoStamp> {
        let t = t.trim();
        let (d, h) = t.split_once(' ').unwrap_or((t, "00:00"));
        let mut dp = d.split('-').map(|x| x.trim().parse::<u16>().ok());
        let mut hp = h.trim().split(':').map(|x| x.trim().parse::<u8>().ok());
        let st = IsoStamp { year: dp.next()??, month: dp.next()?? as u8, day: dp.next()?? as u8, hour: hp.next()??, minute: hp.next().flatten().unwrap_or(0) };
        (st.clamped() == st).then_some(st)
    }
    pub fn text(&self) -> String { format!("{:04}-{:02}-{:02} {:02}:{:02}", self.year, self.month, self.day, self.hour, self.minute) }
    /// Maintenant (temps universel, a la minute).
    pub fn now() -> IsoStamp { let t = Stamp::now(false); IsoStamp { year: t.year as u16, month: t.month, day: t.day, hour: t.hour, minute: t.minute } }
}

/// Date inscrite dans l'image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsoDate {
    /// date et heure de la construction
    Now,
    /// 1er janvier 2000 : le meme dossier donne exactement la meme image
    Fixed,
    /// date choisie (image reproductible elle aussi)
    Custom(IsoStamp),
}

/// Revision UDF ecrite dans l'image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdfRev { V250, V260 }

impl UdfRev {
    fn code(self) -> u16 { match self { UdfRev::V250 => 0x0250, UdfRev::V260 => 0x0260 } }
}

/// Reglages de l'image.
#[derive(Debug, Clone, PartialEq)]
pub struct IsoOptions {
    /// Nom du volume (affiche a l'insertion du disque).
    pub label: String,
    /// Informations du volume (facultatives) : proprietaire, organisation, contact.
    pub owner: String,
    pub organization: String,
    pub contact: String,
    pub udf: UdfRev,
    /// Videos (.m2ts) alignees sur des blocs de 64 Ko.
    pub align: bool,
    /// Ordre des fichiers : navigation et menus, puis videos dans l'ordre, puis copies de secours.
    pub play_order: bool,
    /// Copie miroir des metadonnees (arborescence lisible meme si sa premiere copie est abimee).
    pub mirror: bool,
    /// Date inscrite dans l'image (creation du volume et des fichiers).
    pub date: IsoDate,
    /// Compatibilite maximale : troisieme ancre (secteur N-257) et sequence de reserve des descripteurs en fin de disque.
    pub compat: bool,
}

impl Default for IsoOptions {
    fn default() -> Self {
        IsoOptions { label: String::new(), owner: String::new(), organization: String::new(), contact: String::new(), udf: UdfRev::V250, align: true, play_order: true, mirror: true, date: IsoDate::Now, compat: true }
    }
}

#[derive(Debug, Clone)]
pub struct IsoReport {
    pub bytes: u64,
    pub files: usize,
    pub dirs: usize,
}

fn bad<T>(m: impl Into<String>) -> Result<T> { Err(Error::Format(m.into())) }

// ------------------------------------------------------------------ arborescence source

struct Node {
    name: String,
    src: PathBuf,
    dir: bool,
    size: u64,
    children: Vec<usize>,
    parent: usize,
    uid: u64,
    /// bloc de l'entree de fichier (partition de metadonnees)
    efe: u32,
    /// repertoire : bloc et longueur de ses descripteurs (partition de metadonnees)
    dir_block: u32,
    dir_len: u32,
    /// fichier : premier bloc des donnees (partition physique)
    data: u32,
}

/// Dossiers du disque repris dans l'image (le reste du dossier de sortie est ignore).
const TOP: [&str; 2] = ["BDMV", "CERTIFICATE"];

fn scan(src: &Path) -> Result<Vec<Node>> {
    if !src.join("BDMV").is_dir() { return bad(trf!("no BDMV folder in {}", src.display())); }
    let mut nodes = vec![Node { name: String::new(), src: src.to_path_buf(), dir: true, size: 0, children: vec![], parent: 0, uid: 0, efe: 0, dir_block: 0, dir_len: 0, data: 0 }];
    fn walk(nodes: &mut Vec<Node>, k: usize, top: bool) -> Result<()> {
        let mut entries: Vec<(String, PathBuf, bool, u64)> = Vec::new();
        for e in std::fs::read_dir(&nodes[k].src)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            if top && !TOP.contains(&name.as_str()) { continue; }
            let md = std::fs::metadata(e.path())?;
            if !md.is_dir() && !md.is_file() { continue; }
            entries.push((name, e.path(), md.is_dir(), if md.is_dir() { 0 } else { md.len() }));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path, dir, size) in entries {
            if name.encode_utf16().count() > 254 { return bad(trf!("name too long: {}", name)); }
            let c = nodes.len();
            nodes.push(Node { name, src: path, dir, size, children: vec![], parent: k, uid: 0, efe: 0, dir_block: 0, dir_len: 0, data: 0 });
            nodes[k].children.push(c);
            if dir { walk(nodes, c, false)?; }
        }
        Ok(())
    }
    walk(&mut nodes, 0, true)?;
    Ok(nodes)
}

/// Chemin dans l'image (« BDMV/STREAM/00000.m2ts »).
fn rel(nodes: &[Node], mut k: usize) -> String {
    let mut parts = Vec::new();
    while k != 0 { parts.push(nodes[k].name.as_str()); k = nodes[k].parent; }
    parts.reverse();
    parts.join("/")
}

fn dfs(nodes: &[Node]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![0usize];
    while let Some(k) = stack.pop() {
        out.push(k);
        for &c in nodes[k].children.iter().rev() { stack.push(c); }
    }
    out
}

fn is_stream(path: &str) -> bool { path.to_ascii_lowercase().ends_with(".m2ts") }

/// Rang d'un fichier dans l'ordre « lecture » : navigation et menus, videos, copies de secours, certificats.
fn order_class(path: &str) -> u8 {
    let p = path.to_ascii_uppercase();
    if p.starts_with("BDMV/BACKUP/") { 2 } else if p.starts_with("CERTIFICATE/") { 3 } else if is_stream(&p) { 1 } else { 0 }
}

// ------------------------------------------------------------------ encodage UDF

fn crc16(d: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in d {
        crc ^= (b as u16) << 8;
        for _ in 0..8 { crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 }; }
    }
    crc
}

fn put16(b: &mut [u8], o: usize, v: u16) { b[o..o + 2].copy_from_slice(&v.to_le_bytes()); }
fn put32(b: &mut [u8], o: usize, v: u32) { b[o..o + 4].copy_from_slice(&v.to_le_bytes()); }
fn put64(b: &mut [u8], o: usize, v: u64) { b[o..o + 8].copy_from_slice(&v.to_le_bytes()); }
fn get16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn get32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) }
fn get64(b: &[u8], o: usize) -> u64 { u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) }

/// Etiquette de descripteur (16 octets) : identifiant, version 3, somme de controle, CRC des `crc_len` octets suivants, position.
fn tag(b: &mut [u8], id: u16, loc: u32, crc_len: usize) {
    put16(b, 0, id);
    put16(b, 2, 3);
    b[5] = 0;
    put16(b, 6, 0);
    let crc = crc16(&b[16..16 + crc_len]);
    put16(b, 8, crc);
    put16(b, 10, crc_len as u16);
    put32(b, 12, loc);
    b[4] = 0;
    b[4] = (0..16).filter(|i| *i != 4).fold(0u8, |s, i| s.wrapping_add(b[i]));
}

/// Chaine OSTA CS0 : compression 8 (Latin-1) si possible, sinon 16 (UTF-16BE).
fn cs0(s: &str, max_bytes: usize) -> Vec<u8> {
    if s.is_empty() { return vec![]; }
    if s.chars().all(|c| (c as u32) < 256) {
        let mut v = vec![8u8];
        v.extend(s.chars().take(max_bytes - 1).map(|c| c as u8));
        v
    } else {
        let mut v = vec![16u8];
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut n = ((max_bytes - 1) / 2).min(units.len());
        // ne pas couper une paire de substitution
        if n > 0 && n < units.len() && (0xD800..0xDC00).contains(&units[n - 1]) { n -= 1; }
        for u in &units[..n] { v.extend(u.to_be_bytes()); }
        v
    }
}

fn cs0_decode(b: &[u8]) -> String {
    match b.first() {
        Some(8) => b[1..].iter().map(|&c| c as char).collect(),
        Some(16) => String::from_utf16_lossy(&b[1..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()),
        _ => String::new(),
    }
}

/// Champ dstring de longueur fixe : texte CS0 puis, dans le dernier octet, la longueur utilisee.
fn dstring(b: &mut [u8], o: usize, len: usize, s: &str) {
    let v = cs0(s, len - 1);
    b[o..o + v.len()].copy_from_slice(&v);
    if !v.is_empty() { b[o + len - 1] = v.len() as u8; }
}

fn dstring_decode(b: &[u8]) -> String {
    let n = *b.last().unwrap_or(&0) as usize;
    if n == 0 || n >= b.len() { return String::new(); }
    cs0_decode(&b[..n])
}

fn charspec(b: &mut [u8], o: usize) {
    b[o] = 0;
    let s = b"OSTA Compressed Unicode";
    b[o + 1..o + 1 + s.len()].copy_from_slice(s);
}

/// Identifiant d'entite (32 octets) ; `suffix` : 8 octets.
fn entity(b: &mut [u8], o: usize, id: &str, suffix: [u8; 8]) {
    b[o] = 0;
    b[o + 1..o + 1 + id.len()].copy_from_slice(id.as_bytes());
    b[o + 24..o + 32].copy_from_slice(&suffix);
}

fn udf_suffix(rev: u16) -> [u8; 8] { let r = rev.to_le_bytes(); [r[0], r[1], 0, 0, 0, 0, 0, 0] }
const IMPL: &str = "*bdauthor";
const IMPL_SUFFIX: [u8; 8] = [0; 8];

#[derive(Clone, Copy)]
struct Stamp { year: i16, month: u8, day: u8, hour: u8, minute: u8, second: u8 }

impl Stamp {
    fn of(d: IsoDate) -> Stamp {
        match d {
            IsoDate::Now => Stamp::now(false),
            IsoDate::Fixed => Stamp::now(true),
            IsoDate::Custom(c) => { let c = c.clamped(); Stamp { year: c.year as i16, month: c.month, day: c.day, hour: c.hour, minute: c.minute, second: 0 } }
        }
    }
    fn now(fixed: bool) -> Stamp {
        if fixed { return Stamp { year: 2000, month: 1, day: 1, hour: 0, minute: 0, second: 0 }; }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
        // jours depuis 1970 -> date civile (algorithme de H. Hinnant)
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i16;
        Stamp { year, month, day, hour: (rem / 3600) as u8, minute: (rem / 60 % 60) as u8, second: (rem % 60) as u8 }
    }
    /// Horodatage UDF (12 octets), temps universel.
    fn put(&self, b: &mut [u8], o: usize) {
        put16(b, o, 0x1000);
        put16(b, o + 2, self.year as u16);
        b[o + 4] = self.month;
        b[o + 5] = self.day;
        b[o + 6] = self.hour;
        b[o + 7] = self.minute;
        b[o + 8] = self.second;
        b[o + 9] = 0;
        b[o + 10] = 0;
        b[o + 11] = 0;
    }
}

fn long_ad(b: &mut [u8], o: usize, len: u32, block: u32, part: u16, uid: u32) {
    put32(b, o, len);
    put32(b, o + 4, block);
    put16(b, o + 8, part);
    // ADImpUse : drapeaux (2 octets) puis identifiant unique UDF (4 octets)
    put16(b, o + 10, 0);
    put32(b, o + 12, uid);
}

const T_PVD: u16 = 1;
const T_AVDP: u16 = 2;
const T_IUVD: u16 = 4;
const T_PD: u16 = 5;
const T_LVD: u16 = 6;
const T_USD: u16 = 7;
const T_TD: u16 = 8;
const T_LVID: u16 = 9;
const T_FSD: u16 = 256;
const T_FID: u16 = 257;
const T_FE: u16 = 261;
const T_EFE: u16 = 266;

const FT_DIR: u8 = 4;
const FT_FILE: u8 = 5;
const FT_META: u8 = 250;
const FT_MIRROR: u8 = 251;

/// Longueur d'un descripteur d'identifiant de fichier (multiple de 4).
fn fid_len(name: &str) -> usize {
    let l_fi = if name.is_empty() { 0 } else { cs0(name, 255).len() };
    (38 + l_fi + 3) & !3
}

fn fid(out: &mut Vec<u8>, base_block: u32, name: &str, dir: bool, parent: bool, efe: u32, uid: u64) {
    let start = out.len();
    let n = fid_len(name);
    out.resize(start + n, 0);
    let ident = if name.is_empty() { vec![] } else { cs0(name, 255) };
    let b = &mut out[start..];
    put16(b, 16, 1);
    b[18] = (if dir { 0x02 } else { 0 }) | (if parent { 0x08 } else { 0 });
    b[19] = ident.len() as u8;
    long_ad(b, 20, BS as u32, efe, 1, uid as u32);
    put16(b, 36, 0);
    b[38..38 + ident.len()].copy_from_slice(&ident);
    tag(b, T_FID, base_block + (start / BS) as u32, n - 16);
}

struct EfeData<'a> {
    loc: u32,
    file_type: u8,
    links: u16,
    size: u64,
    uid: u64,
    /// (descripteurs courts dans la partition de l'entree) ou (descripteurs longs : bloc, longueur, partition)
    short: &'a [(u32, u32)],
    long: &'a [(u32, u32, u16)],
}

/// Entree de fichier etendue (EFE), un bloc.
fn efe(e: &EfeData, date: &Stamp) -> Result<Vec<u8>> {
    let mut b = vec![0u8; BS];
    let (flags, ad_len) = if !e.long.is_empty() { (1u16, e.long.len() * 16) } else { (0u16, e.short.len() * 8) };
    if 216 + ad_len > BS { return bad(tr("file too large for the image (too many extents)")); }
    // ICB tag
    put32(&mut b, 16, 0);
    put16(&mut b, 20, 4);
    put16(&mut b, 22, 0);
    put16(&mut b, 24, 1);
    b[27] = e.file_type;
    put16(&mut b, 34, flags);
    put32(&mut b, 36, u32::MAX);
    put32(&mut b, 40, u32::MAX);
    // lecture et execution pour tous
    put32(&mut b, 44, 0x14A5);
    put16(&mut b, 48, e.links);
    put64(&mut b, 56, e.size);
    put64(&mut b, 64, e.size);
    put64(&mut b, 72, e.size.div_ceil(SECTOR));
    for o in [80, 92, 104, 116] { date.put(&mut b, o); }
    put32(&mut b, 128, 1);
    entity(&mut b, 168, IMPL, IMPL_SUFFIX);
    put64(&mut b, 200, e.uid);
    put32(&mut b, 208, 0);
    put32(&mut b, 212, ad_len as u32);
    let mut o = 216;
    for &(block, len) in e.short { put32(&mut b, o, len); put32(&mut b, o + 4, block); o += 8; }
    for &(block, len, part) in e.long { long_ad(&mut b, o, len, block, part, 0); o += 16; }
    tag(&mut b, T_EFE, e.loc, 216 + ad_len - 16);
    Ok(b)
}

/// Decoupe `size` octets a partir de `block` en etendues d'au plus `max` octets.
fn extents(block: u32, size: u64, max: u32) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    let (mut b, mut left) = (block, size);
    while left > 0 {
        let n = left.min(max as u64) as u32;
        v.push((b, n));
        b += n.div_ceil(BS as u32);
        left -= n as u64;
    }
    v
}

fn round_up(v: u32, m: u32) -> u32 { v.div_ceil(m) * m }

// ------------------------------------------------------------------ plan de l'image

struct Plan {
    nodes: Vec<Node>,
    /// fichiers dans l'ordre d'ecriture
    order: Vec<usize>,
    meta: Vec<u8>,
    /// blocs (partition physique) : contenu des metadonnees, copie miroir
    meta_at: u32,
    mirror_at: u32,
    meta_blocks: u32,
    part_len: u32,
    next_uid: u64,
    files: usize,
    dirs: usize,
}

fn plan(src: &Path, opt: &IsoOptions, max_extent: u32, date: &Stamp) -> Result<Plan> {
    let mut nodes = scan(src)?;
    let order_tree = dfs(&nodes);
    // identifiants uniques : racine 0, puis 16..
    let mut next_uid = 16u64;
    for &k in &order_tree[1..] { nodes[k].uid = next_uid; next_uid += 1; }
    // partition de metadonnees : 0 FSD, 1 TD, puis entree et descripteurs de chaque noeud
    let mut mb = 2u32;
    for &k in &order_tree {
        nodes[k].efe = mb;
        mb += 1;
        if nodes[k].dir {
            let len = fid_len("") + nodes[k].children.iter().map(|&c| fid_len(&nodes[c].name)).sum::<usize>();
            nodes[k].dir_block = mb;
            nodes[k].dir_len = len as u32;
            mb += (len as u32).div_ceil(BS as u32);
        }
    }
    let meta_blocks = round_up(mb, ALIGN);
    // partition physique : 0 entree des metadonnees, 1 entree du miroir, 32.. metadonnees, puis donnees
    let meta_at = ALIGN;
    let mut cur = meta_at + meta_blocks;
    let mut order: Vec<usize> = order_tree.iter().copied().filter(|&k| !nodes[k].dir).collect();
    if opt.play_order {
        let paths: Vec<(u8, usize)> = order.iter().map(|&k| (order_class(&rel(&nodes, k)), k)).collect();
        let mut p = paths;
        p.sort_by_key(|&(c, _)| c); // tri stable : ordre de l'arborescence dans chaque groupe
        order = p.into_iter().map(|(_, k)| k).collect();
    }
    for &k in &order {
        if nodes[k].size == 0 { continue; }
        if opt.align && is_stream(&nodes[k].name) { cur = round_up(cur, ALIGN); }
        nodes[k].data = cur;
        let blocks = nodes[k].size.div_ceil(SECTOR);
        if cur as u64 + blocks > u32::MAX as u64 / 2 { return bad(tr("image too large")); }
        cur += blocks as u32;
    }
    let mirror_at = if opt.mirror { let m = round_up(cur, ALIGN); cur = m + meta_blocks; m } else { meta_at };
    let part_len = round_up(cur, ALIGN);
    // contenu des metadonnees
    let mut meta = vec![0u8; meta_blocks as usize * BS];
    {
        let b = &mut meta[..BS];
        date.put(b, 16);
        put16(b, 28, 3);
        put16(b, 30, 3);
        put32(b, 32, 1);
        put32(b, 36, 1);
        charspec(b, 48);
        dstring(b, 112, 128, &opt.label);
        charspec(b, 240);
        dstring(b, 304, 32, &opt.label);
        long_ad(b, 400, BS as u32, nodes[0].efe, 1, 0);
        entity(b, 416, "*OSTA UDF Compliant", udf_suffix(opt.udf.code()));
        tag(b, T_FSD, 0, 512 - 16);
        let t = &mut meta[BS..2 * BS];
        tag(t, T_TD, 1, 512 - 16);
    }
    let (mut files, mut dirs) = (0, 0);
    for &k in &order_tree {
        let n = &nodes[k];
        let block = if n.dir {
            dirs += 1;
            // descripteurs du repertoire : parent d'abord (la racine est son propre parent)
            let mut d = Vec::with_capacity(n.dir_len as usize);
            let p = &nodes[n.parent];
            fid(&mut d, n.dir_block, "", true, true, p.efe, p.uid);
            for &c in &n.children { fid(&mut d, n.dir_block, &nodes[c].name, nodes[c].dir, false, nodes[c].efe, nodes[c].uid); }
            debug_assert_eq!(d.len(), n.dir_len as usize);
            let o = n.dir_block as usize * BS;
            meta[o..o + d.len()].copy_from_slice(&d);
            let subdirs = n.children.iter().filter(|&&c| nodes[c].dir).count() as u16;
            efe(&EfeData { loc: n.efe, file_type: FT_DIR, links: 1 + subdirs, size: n.dir_len as u64, uid: n.uid, short: &[(n.dir_block, n.dir_len)], long: &[] }, date)?
        } else {
            files += 1;
            let ext: Vec<(u32, u32, u16)> = extents(n.data, n.size, max_extent).into_iter().map(|(b, l)| (b, l, 0)).collect();
            efe(&EfeData { loc: n.efe, file_type: FT_FILE, links: 1, size: n.size, uid: n.uid, short: &[], long: &ext }, date)?
        };
        let o = n.efe as usize * BS;
        meta[o..o + BS].copy_from_slice(&block);
    }
    Ok(Plan { nodes, order, meta, meta_at, mirror_at, meta_blocks, part_len, next_uid, files, dirs })
}

/// Taille de l'image qu'on obtiendrait (octets), sans l'ecrire.
pub fn iso_size(src: &Path, opt: &IsoOptions) -> Result<u64> {
    let p = plan(src, opt, MAX_EXTENT, &Stamp::now(true))?;
    Ok(layout(&p, opt).0 * SECTOR)
}

/// Fin de l'image en mode compatibilite : sequence de reserve a +16, ancres a N-257 (+31) et sur le dernier secteur.
const TAIL: u32 = 288;
const TAIL_RESERVE: u32 = 16;

/// (nombre total de secteurs, secteur de la sequence de reserve).
fn layout(p: &Plan, opt: &IsoOptions) -> (u64, u32) {
    let end = PART_START + p.part_len;
    if opt.compat { ((end + TAIL) as u64, end + TAIL_RESERVE) } else { ((end + 1) as u64, 48) }
}

// ------------------------------------------------------------------ descripteurs de volume

/// Zone des descripteurs (secteurs 0..PART_START) et sequence de reserve (6 secteurs, a placer a `reserve`).
fn volume_area(p: &Plan, opt: &IsoOptions, date: &Stamp, reserve: u32) -> (Vec<u8>, Vec<u8>) {
    let rev = opt.udf.code();
    let mut a = vec![0u8; PART_START as usize * BS];
    // sequence de reconnaissance
    for (i, id) in [b"BEA01", b"NSR03", b"TEA01"].iter().enumerate() {
        let o = (16 + i) * BS;
        a[o] = 0;
        a[o + 1..o + 6].copy_from_slice(*id);
        a[o + 6] = 1;
    }
    // identifiant de l'ensemble de volumes : 16 chiffres hexadecimaux uniques (date, ou empreinte du contenu si la date est
    // fixe ou choisie : image reproductible)
    let uniq: u64 = if opt.date != IsoDate::Now {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for n in &p.nodes { for b in n.name.bytes().chain(n.size.to_le_bytes()) { h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3); } }
        h
    } else {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
    };
    let vsid = format!("{uniq:016X}{}", opt.label);
    let sequence = |start: u32| -> Vec<u8> {
        let mut a = vec![0u8; 6 * BS];
        let s = |i: u32| i as usize * BS;
        // PVD
        {
            let b = &mut a[s(0)..s(0) + BS];
            put32(b, 16, 0);
            put32(b, 20, 0);
            dstring(b, 24, 32, &opt.label);
            put16(b, 56, 1);
            put16(b, 58, 1);
            put16(b, 60, 2);
            put16(b, 62, 3);
            put32(b, 64, 1);
            put32(b, 68, 1);
            dstring(b, 72, 128, &vsid);
            charspec(b, 200);
            charspec(b, 264);
            entity(b, 344, IMPL, IMPL_SUFFIX);
            date.put(b, 376);
            entity(b, 388, IMPL, IMPL_SUFFIX);
            tag(b, T_PVD, start, 512 - 16);
        }
        // IUVD : informations du volume logique
        {
            let b = &mut a[s(1)..s(1) + BS];
            put32(b, 16, 1);
            entity(b, 20, "*UDF LV Info", udf_suffix(rev));
            charspec(b, 52);
            dstring(b, 116, 128, &opt.label);
            dstring(b, 244, 36, &opt.owner);
            dstring(b, 280, 36, &opt.organization);
            dstring(b, 316, 36, &opt.contact);
            entity(b, 352, IMPL, IMPL_SUFFIX);
            tag(b, T_IUVD, start + 1, 512 - 16);
        }
        // PD : partition physique, lecture seule
        {
            let b = &mut a[s(2)..s(2) + BS];
            put32(b, 16, 2);
            put16(b, 20, 1);
            put16(b, 22, 0);
            entity(b, 24, "+NSR03", [0; 8]);
            put32(b, 184, 1);
            put32(b, 188, PART_START);
            put32(b, 192, p.part_len);
            entity(b, 196, IMPL, IMPL_SUFFIX);
            tag(b, T_PD, start + 2, 512 - 16);
        }
        // LVD : volume logique, carte de la partition physique + carte de la partition de metadonnees
        {
            let b = &mut a[s(3)..s(3) + BS];
            put32(b, 16, 3);
            charspec(b, 20);
            dstring(b, 84, 128, &opt.label);
            put32(b, 212, BS as u32);
            entity(b, 216, "*OSTA UDF Compliant", udf_suffix(rev));
            long_ad(b, 248, 2 * BS as u32, 0, 1, 0);
            put32(b, 264, 6 + 64);
            put32(b, 268, 2);
            entity(b, 272, IMPL, IMPL_SUFFIX);
            put32(b, 432, 2 * BS as u32);
            put32(b, 436, 64);
            // carte de type 1
            b[440] = 1;
            b[441] = 6;
            put16(b, 442, 1);
            put16(b, 444, 0);
            // carte de type 2 : partition de metadonnees
            let m = 446;
            b[m] = 2;
            b[m + 1] = 64;
            entity(b, m + 4, "*UDF Metadata Partition", udf_suffix(rev));
            put16(b, m + 36, 1);
            put16(b, m + 38, 0);
            put32(b, m + 40, 0); // entree du fichier de metadonnees
            put32(b, m + 44, 1); // entree du miroir
            put32(b, m + 48, u32::MAX); // pas de fichier bitmap (lecture seule)
            put32(b, m + 52, ALIGN);
            put16(b, m + 56, ALIGN as u16);
            b[m + 58] = if opt.mirror { 1 } else { 0 };
            tag(b, T_LVD, start + 3, 440 + 70 - 16);
        }
        // USD : pas d'espace non alloue
        {
            let b = &mut a[s(4)..s(4) + BS];
            put32(b, 16, 4);
            put32(b, 20, 0);
            tag(b, T_USD, start + 4, 8);
        }
        let b = &mut a[s(5)..s(5) + BS];
        tag(b, T_TD, start + 5, 512 - 16);
        a
    };
    a[32 * BS..38 * BS].copy_from_slice(&sequence(32));
    let res = sequence(reserve);
    if reserve == 48 { a[48 * BS..54 * BS].copy_from_slice(&res); }
    // LVID : volume ferme
    {
        let o = 64 * BS;
        let b = &mut a[o..o + BS];
        date.put(b, 16);
        put32(b, 28, 1);
        put64(b, 40, p.next_uid);
        put32(b, 72, 2);
        put32(b, 76, 46);
        put32(b, 80, 0);
        put32(b, 84, 0);
        put32(b, 88, p.part_len);
        put32(b, 92, p.meta_blocks);
        let iu = 96;
        entity(b, iu, IMPL, IMPL_SUFFIX);
        put32(b, iu + 32, p.files as u32);
        put32(b, iu + 36, p.dirs as u32);
        put16(b, iu + 40, rev);
        put16(b, iu + 42, rev);
        put16(b, iu + 44, rev);
        tag(b, T_LVID, 64, 80 + 16 + 46 - 16);
        let t = &mut a[65 * BS..66 * BS];
        tag(t, T_TD, 65, 512 - 16);
    }
    a[256 * BS..257 * BS].copy_from_slice(&avdp(256, reserve));
    (a, res)
}

fn avdp(loc: u32, reserve: u32) -> Vec<u8> {
    let mut b = vec![0u8; BS];
    put32(&mut b, 16, 16 * BS as u32);
    put32(&mut b, 20, 32);
    put32(&mut b, 24, 16 * BS as u32);
    put32(&mut b, 28, reserve);
    tag(&mut b, T_AVDP, loc, 512 - 16);
    b
}

// ------------------------------------------------------------------ ecriture

/// Ecrit l'image du dossier `src` (qui contient BDMV et CERTIFICATE) dans `out`. `progress` : 0.0..=1.0.
pub fn write_iso(src: &Path, out: &Path, opt: &IsoOptions, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<IsoReport> {
    write_iso_ext(src, out, opt, MAX_EXTENT, cancel, progress)
}

/// Comme `write_iso`, avec une longueur d'etendue maximale reglable (tests du decoupage des gros fichiers).
#[doc(hidden)]
pub fn write_iso_ext(src: &Path, out: &Path, opt: &IsoOptions, max_extent: u32, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<IsoReport> {
    let max_extent = (max_extent / BS as u32).max(1) * BS as u32;
    let date = Stamp::of(opt.date);
    let p = plan(src, opt, max_extent, &date)?;
    let (total_sectors, reserve) = layout(&p, opt);
    let total = total_sectors * SECTOR;
    let tmp = out.with_extension("iso.part");
    let res = (|| -> Result<()> {
        let mut w = BufWriter::with_capacity(4 << 20, File::create(&tmp)?);
        let (area, reserve_seq) = volume_area(&p, opt, &date, reserve);
        w.write_all(&area)?;
        // partition physique
        let zeros = vec![0u8; BS * 64];
        let pad_to = |w: &mut BufWriter<File>, cur: &mut u32, to: u32| -> Result<()> {
            let mut n = (to - *cur) as usize * BS;
            while n > 0 { let k = n.min(zeros.len()); w.write_all(&zeros[..k])?; n -= k; }
            *cur = to;
            Ok(())
        };
        let meta_ext = [(p.meta_at, p.meta_blocks * BS as u32)];
        let mirror_ext = [(p.mirror_at, p.meta_blocks * BS as u32)];
        let common = |loc, ft, short: &[(u32, u32)]| efe(&EfeData { loc, file_type: ft, links: 1, size: p.meta_blocks as u64 * SECTOR, uid: 0, short, long: &[] }, &date);
        w.write_all(&common(0, FT_META, &meta_ext)?)?;
        w.write_all(&common(1, FT_MIRROR, &mirror_ext)?)?;
        let mut cur: u32 = 2; // bloc courant
        pad_to(&mut w, &mut cur, p.meta_at)?;
        w.write_all(&p.meta)?;
        cur += p.meta_blocks;
        let data_total: u64 = p.order.iter().map(|&k| p.nodes[k].size).sum::<u64>().max(1);
        let mut done = 0u64;
        let mut buf = vec![0u8; 1 << 20];
        for &k in &p.order {
            let n = &p.nodes[k];
            if n.size == 0 { continue; }
            pad_to(&mut w, &mut cur, n.data)?;
            let mut f = File::open(&n.src)?;
            let mut left = n.size;
            while left > 0 {
                if cancel.load(Ordering::Relaxed) { return Err(Error::Cancelled); }
                let want = left.min(buf.len() as u64) as usize;
                let got = f.read(&mut buf[..want])?;
                if got == 0 { return bad(trf!("{} changed during image creation", rel(&p.nodes, k))); }
                w.write_all(&buf[..got])?;
                left -= got as u64;
                done += got as u64;
                progress(done as f64 / data_total as f64 * 0.999);
            }
            let rem = (n.size % SECTOR) as usize;
            if rem != 0 { w.write_all(&zeros[..BS - rem])?; }
            cur += n.size.div_ceil(SECTOR) as u32;
        }
        if opt.mirror {
            pad_to(&mut w, &mut cur, p.mirror_at)?;
            w.write_all(&p.meta)?;
            cur += p.meta_blocks;
        }
        pad_to(&mut w, &mut cur, p.part_len)?;
        if opt.compat {
            // fin du disque : sequence de reserve, ancre a N-257, ancre sur le dernier secteur
            let end = PART_START + p.part_len;
            let mut tail = vec![0u8; TAIL as usize * BS];
            tail[TAIL_RESERVE as usize * BS..(TAIL_RESERVE as usize + 6) * BS].copy_from_slice(&reserve_seq);
            let n = total_sectors as u32;
            let o = (n - 257 - end) as usize * BS;
            tail[o..o + BS].copy_from_slice(&avdp(n - 257, reserve));
            tail[(TAIL as usize - 1) * BS..].copy_from_slice(&avdp(n - 1, reserve));
            w.write_all(&tail)?;
        } else {
            w.write_all(&avdp((total_sectors - 1) as u32, reserve))?;
        }
        w.flush()?;
        let f = w.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        f.sync_all()?;
        if f.metadata()?.len() != total { return bad(tr("unexpected image size")); }
        Ok(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if out.exists() { std::fs::remove_file(out)?; }
    std::fs::rename(&tmp, out)?;
    progress(1.0);
    Ok(IsoReport { bytes: total, files: p.files, dirs: p.dirs })
}

// ------------------------------------------------------------------ lecture (verification)

/// Entree lue dans une image.
#[derive(Debug, Clone)]
pub struct IsoEntry {
    pub path: String,
    pub dir: bool,
    pub size: u64,
    /// etendues des donnees : (secteur absolu, longueur en octets)
    pub extents: Vec<(u64, u64)>,
}

/// Contenu d'une image : volume et arborescence.
#[derive(Debug, Clone)]
pub struct IsoTree {
    pub label: String,
    pub owner: String,
    pub organization: String,
    pub contact: String,
    pub udf_rev: u16,
    pub mirror: bool,
    /// le miroir est-il une copie distincte, identique aux metadonnees ?
    pub mirror_ok: bool,
    pub sectors: u64,
    /// ancres trouvees (secteur 256, N-257 si present, dernier secteur)
    pub anchors: u8,
    /// secteur de la sequence de reserve des descripteurs de volume
    pub reserve_at: u32,
    pub entries: Vec<IsoEntry>,
}

struct Reader {
    f: File,
    sectors: u64,
    part_start: u64,
    part_len: u64,
    meta: Vec<(u64, u64)>, // etendues du fichier de metadonnees (bloc physique, octets)
}

fn check_tag(b: &[u8], id: u16, loc: u32, what: &str) -> Result<()> {
    if b.len() < 16 { return bad(format!("{what} : descripteur tronque")); }
    let sum = (0..16).filter(|i| *i != 4).fold(0u8, |s, i| s.wrapping_add(b[i]));
    if sum != b[4] { return bad(format!("{what} : somme de controle de l'etiquette invalide")); }
    if get16(b, 0) != id { return bad(format!("{what} : descripteur {} au lieu de {id}", get16(b, 0))); }
    let n = get16(b, 10) as usize;
    if 16 + n > b.len() || crc16(&b[16..16 + n]) != get16(b, 8) { return bad(format!("{what} : CRC invalide")); }
    if get32(b, 12) != loc { return bad(format!("{what} : position {} au lieu de {loc}", get32(b, 12))); }
    Ok(())
}

impl Reader {
    fn sector(&mut self, s: u64) -> Result<Vec<u8>> {
        if s >= self.sectors { return bad(format!("secteur {s} hors de l'image")); }
        let mut b = vec![0u8; BS];
        self.f.seek(SeekFrom::Start(s * SECTOR))?;
        self.f.read_exact(&mut b)?;
        Ok(b)
    }
    /// Secteur absolu d'un bloc logique (`part` : 0 physique, 1 metadonnees).
    fn abs(&self, part: u16, block: u32) -> Result<u64> {
        match part {
            0 => {
                if block as u64 >= self.part_len { return bad(format!("bloc {block} hors de la partition")); }
                Ok(self.part_start + block as u64)
            }
            1 => {
                let mut off = block as u64 * SECTOR;
                for &(b, len) in &self.meta {
                    if off < len { return self.abs(0, (b + off / SECTOR) as u32); }
                    off -= len;
                }
                bad(format!("bloc de metadonnees {block} hors du fichier de metadonnees"))
            }
            _ => bad(format!("reference de partition {part} inconnue")),
        }
    }
    fn block(&mut self, part: u16, block: u32) -> Result<Vec<u8>> { let s = self.abs(part, block)?; self.sector(s) }
    fn read_extents(&mut self, part: u16, ext: &[(u32, u32)]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for &(b, len) in ext {
            for i in 0..len.div_ceil(BS as u32) {
                let s = self.block(part, b + i)?;
                let take = (len as usize - i as usize * BS).min(BS);
                out.extend_from_slice(&s[..take]);
            }
        }
        Ok(out)
    }
}

/// Entree de fichier lue : type, taille, donnees integrees ou etendues (partition, bloc, longueur).
struct Fe { file_type: u8, size: u64, embedded: Option<Vec<u8>>, ext: Vec<(u16, u32, u32)> }

fn parse_fe(b: &[u8], part: u16, loc: u32, what: &str) -> Result<Fe> {
    let id = get16(b, 0);
    let (ea_off, ad_off) = match id { T_EFE => (208, 216), T_FE => (168, 176), _ => return bad(format!("{what} : entree de fichier attendue (descripteur {id})")) };
    check_tag(b, id, loc, what)?;
    let file_type = b[27];
    let flags = get16(b, 34) & 7;
    let size = get64(b, 56);
    let l_ea = get32(b, ea_off) as usize;
    let l_ad = get32(b, ea_off + 4) as usize;
    let start = ad_off + l_ea;
    if start + l_ad > b.len() { return bad(format!("{what} : descripteurs d'allocation hors du bloc")); }
    let ads = &b[start..start + l_ad];
    let mut ext = Vec::new();
    let mut embedded = None;
    match flags {
        0 => for c in ads.chunks_exact(8) { let l = get32(c, 0); if l & 0x3FFF_FFFF == 0 { break; } if l >> 30 != 0 { return bad(format!("{what} : etendue non enregistree")); } ext.push((part, get32(c, 4), l)); },
        1 => for c in ads.chunks_exact(16) { let l = get32(c, 0); if l & 0x3FFF_FFFF == 0 { break; } if l >> 30 != 0 { return bad(format!("{what} : etendue non enregistree")); } ext.push((get16(c, 8), get32(c, 4), l)); },
        3 => embedded = Some(ads.to_vec()),
        f => return bad(format!("{what} : type d'allocation {f} non gere")),
    }
    let recorded: u64 = ext.iter().map(|e| e.2 as u64).sum();
    if embedded.is_none() && recorded != size { return bad(format!("{what} : taille {size} mais {recorded} octets alloues")); }
    Ok(Fe { file_type, size, embedded, ext })
}

/// Lit et controle la structure d'une image (etiquettes, CRC, positions, arborescence).
pub fn read_iso(path: &Path) -> Result<IsoTree> {
    let f = File::open(path)?;
    let len = f.metadata()?.len();
    if len % SECTOR != 0 || len < 300 * SECTOR { return bad("taille d'image invalide"); }
    let mut r = Reader { f, sectors: len / SECTOR, part_start: 0, part_len: 0, meta: vec![] };
    let vrs = r.sector(17)?;
    if &vrs[1..6] != b"NSR03" { return bad("sequence de reconnaissance UDF absente (NSR03)"); }
    let a = r.sector(256)?;
    check_tag(&a, T_AVDP, 256, "ancre (secteur 256)")?;
    let last = r.sectors - 1;
    let a2 = r.sector(last)?;
    check_tag(&a2, T_AVDP, last as u32, "ancre (dernier secteur)")?;
    let mut anchors = 2u8;
    if r.sectors > 257 + 300 {
        let s3 = r.sectors - 257;
        let a3 = r.sector(s3)?;
        if get16(&a3, 0) == T_AVDP { check_tag(&a3, T_AVDP, s3 as u32, "ancre (secteur N-257)")?; anchors = 3; }
    }
    // sequence de reserve : chaque descripteur controle
    let (res_len, res_at) = (get32(&a, 24), get32(&a, 28));
    for i in 0..res_len / BS as u32 {
        let s = res_at + i;
        let b = r.sector(s as u64)?;
        let id = get16(&b, 0);
        check_tag(&b, id, s, &format!("descripteur de reserve (secteur {s})"))?;
        if id == T_TD { break; }
    }
    let (vds_len, vds) = (get32(&a, 16), get32(&a, 20));
    let mut label = String::new();
    let (mut owner, mut organization, mut contact) = (String::new(), String::new(), String::new());
    let mut lvd: Option<Vec<u8>> = None;
    let mut pd_ok = false;
    for i in 0..vds_len / BS as u32 {
        let s = vds + i;
        let b = r.sector(s as u64)?;
        let id = get16(&b, 0);
        check_tag(&b, id, s, &format!("descripteur de volume (secteur {s})"))?;
        match id {
            T_PVD => label = dstring_decode(&b[24..56]),
            T_IUVD => { owner = dstring_decode(&b[244..280]); organization = dstring_decode(&b[280..316]); contact = dstring_decode(&b[316..352]); }
            T_PD => { r.part_start = get32(&b, 188) as u64; r.part_len = get32(&b, 192) as u64; pd_ok = true; }
            T_LVD => lvd = Some(b),
            T_TD => break,
            _ => {}
        }
    }
    let Some(lvd) = lvd else { return bad("descripteur de volume logique absent") };
    if !pd_ok { return bad("descripteur de partition absent"); }
    if r.part_start + r.part_len > r.sectors { return bad("partition hors de l'image"); }
    if get32(&lvd, 212) != BS as u32 { return bad("taille de bloc logique differente de 2048"); }
    let udf_rev = get16(&lvd, 216 + 24);
    let maps = get32(&lvd, 268);
    let mut o = 440;
    let mut meta_map = None;
    for _ in 0..maps {
        let (t, l) = (lvd[o], lvd[o + 1] as usize);
        if t == 2 && &lvd[o + 5..o + 5 + 23] == b"*UDF Metadata Partition" {
            meta_map = Some((get32(&lvd, o + 40), get32(&lvd, o + 44), lvd[o + 58] & 1 != 0));
        }
        o += l.max(1);
    }
    let Some((meta_loc, mirror_loc, mirror)) = meta_map else { return bad("partition de metadonnees absente (UDF 2.50 exige)") };
    let mb = r.block(0, meta_loc)?;
    let meta_fe = parse_fe(&mb, 0, meta_loc, "fichier de metadonnees")?;
    if meta_fe.file_type != FT_META { return bad("type du fichier de metadonnees invalide"); }
    r.meta = meta_fe.ext.iter().map(|e| (e.1 as u64, e.2 as u64)).collect();
    let mm = r.block(0, mirror_loc)?;
    let mirror_fe = parse_fe(&mm, 0, mirror_loc, "miroir des metadonnees")?;
    if mirror_fe.file_type != FT_MIRROR { return bad("type du miroir des metadonnees invalide"); }
    let to_phys = |v: &[(u16, u32, u32)]| v.iter().map(|e| (e.1, e.2)).collect::<Vec<_>>();
    let meta_bytes = r.read_extents(0, &to_phys(&meta_fe.ext))?;
    let mirror_bytes = r.read_extents(0, &to_phys(&mirror_fe.ext))?;
    let distinct = meta_fe.ext.iter().map(|e| e.1).collect::<Vec<_>>() != mirror_fe.ext.iter().map(|e| e.1).collect::<Vec<_>>();
    let mirror_ok = distinct && meta_bytes == mirror_bytes;
    // descripteur d'ensemble de fichiers
    let (fsd_block, fsd_part) = (get32(&lvd, 252), get16(&lvd, 256));
    let fsd = r.block(fsd_part, fsd_block)?;
    check_tag(&fsd, T_FSD, fsd_block, "descripteur d'ensemble de fichiers")?;
    let (root_block, root_part) = (get32(&fsd, 404), get16(&fsd, 408));
    let mut entries = Vec::new();
    let mut stack = vec![(String::new(), root_part, root_block, 0usize)];
    while let Some((path, part, block, depth)) = stack.pop() {
        if depth > 32 { return bad("arborescence trop profonde (boucle ?)"); }
        let b = r.block(part, block)?;
        let fe = parse_fe(&b, part, block, &format!("entree de « {} »", if path.is_empty() { "/" } else { &path }))?;
        let data_ext: Vec<(u64, u64)> = fe.ext.iter().map(|&(p, bl, l)| Ok((r.abs(p, bl)?, l as u64))).collect::<Result<_>>()?;
        if fe.file_type == FT_DIR {
            let data = match &fe.embedded {
                Some(d) => d.clone(),
                None => {
                    let mut d = Vec::new();
                    for &(p, bl, l) in &fe.ext { d.extend(r.read_extents(p, &[(bl, l)])?); }
                    d
                }
            };
            if !path.is_empty() { entries.push(IsoEntry { path: path.clone(), dir: true, size: 0, extents: vec![] }); }
            let first_block = fe.ext.first().map_or(block, |e| e.1);
            let mut o = 0;
            let mut children = Vec::new();
            while o + 38 <= data.len() {
                let d = &data[o..];
                let l_fi = d[19] as usize;
                let l_iu = get16(d, 36) as usize;
                let n = (38 + l_iu + l_fi + 3) & !3;
                if n > d.len() { return bad(format!("« {path} » : descripteur de fichier tronque")); }
                let loc = if fe.embedded.is_some() { block } else { first_block + (o / BS) as u32 };
                check_tag(&d[..n], T_FID, loc, &format!("descripteur de fichier dans « {path} »"))?;
                let chars = d[18];
                if chars & 0x08 == 0 && chars & 0x04 == 0 {
                    let name = cs0_decode(&d[38 + l_iu..38 + l_iu + l_fi]);
                    if name.is_empty() || name.contains('/') { return bad(format!("« {path} » : nom de fichier invalide")); }
                    let child = if path.is_empty() { name } else { format!("{path}/{name}") };
                    children.push((child, get16(d, 28), get32(d, 24), depth + 1));
                }
                o += n;
            }
            children.reverse();
            stack.extend(children);
        } else if fe.file_type == FT_FILE {
            entries.push(IsoEntry { path, dir: false, size: fe.size, extents: data_ext });
        } else {
            return bad(format!("« {path} » : type de fichier {} inattendu", fe.file_type));
        }
    }
    Ok(IsoTree { label, owner, organization, contact, udf_rev, mirror, mirror_ok, sectors: r.sectors, anchors, reserve_at: res_at, entries })
}

/// Verifie une image contre le dossier source : structure, liste des fichiers et contenu octet par octet. Rend le nombre
/// de fichiers compares.
pub fn verify_iso(iso: &Path, src: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<usize> {
    let tree = read_iso(iso)?;
    if tree.mirror && !tree.mirror_ok { return bad("la copie miroir des metadonnees differe de l'original"); }
    let nodes = scan(src)?;
    let mut want: Vec<(String, bool, u64)> = dfs(&nodes)[1..].iter().map(|&k| (rel(&nodes, k), nodes[k].dir, nodes[k].size)).collect();
    let mut got: Vec<(String, bool, u64)> = tree.entries.iter().map(|e| (e.path.clone(), e.dir, e.size)).collect();
    want.sort();
    got.sort();
    if want != got {
        let missing: Vec<&str> = want.iter().filter(|w| !got.contains(w)).map(|w| w.0.as_str()).take(5).collect();
        let extra: Vec<&str> = got.iter().filter(|g| !want.contains(g)).map(|g| g.0.as_str()).take(5).collect();
        return bad(trf!("tree differs from the folder (missing or different: {} ; extra: {})", format!("{:?}", missing), format!("{:?}", extra)));
    }
    let total: u64 = tree.entries.iter().map(|e| e.size).sum::<u64>().max(1);
    let mut done = 0u64;
    let mut f = File::open(iso)?;
    let (mut a, mut b) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    let mut count = 0;
    for e in tree.entries.iter().filter(|e| !e.dir) {
        let mut s = File::open(src.join(&e.path))?;
        for &(sector, len) in &e.extents {
            f.seek(SeekFrom::Start(sector * SECTOR))?;
            let mut left = len;
            while left > 0 {
                if cancel.load(Ordering::Relaxed) { return Err(Error::Cancelled); }
                let n = left.min(a.len() as u64) as usize;
                f.read_exact(&mut a[..n])?;
                s.read_exact(&mut b[..n])?;
                if a[..n] != b[..n] { return bad(trf!("content differs for {}", e.path)); }
                left -= n as u64;
                done += n as u64;
                progress(done as f64 / total as f64);
            }
        }
        count += 1;
    }
    progress(1.0);
    Ok(count)
}

// ------------------------------------------------------------------ reglages du projet (cle « iso »)

/// Ce que la construction laisse sur le disque dur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsoKeep {
    /// dossier BDMV seul (pas d'image)
    Folder,
    /// image seule : le dossier construit est supprime une fois l'image ecrite (et verifiee si demande)
    Iso,
    /// dossier et image
    Both,
}

/// Marge de securite par defaut (% de la capacite) : la gravure est moins fiable tout au bord d'un BD-R.
pub const DEFAULT_MARGIN: u8 = 95;

/// Reglages de la generation ISO enregistres dans le projet (seuls les reglages differents des valeurs par defaut sont ecrits).
#[derive(Debug, Clone, PartialEq)]
pub struct IsoSettings {
    pub keep: IsoKeep,
    /// Fichier de l'image (relatif au projet ou absolu) ; vide : a cote du dossier construit, ou demande a la construction.
    pub file: String,
    /// Capacite visee : rang dans `CAPACITIES`.
    pub capacity: usize,
    /// Reglages de l'image ; nom du volume vide : titre du disque.
    pub opts: IsoOptions,
    /// Verification apres creation.
    pub verify: bool,
    /// Marge de securite : avertir au-dela de ce pourcentage de la capacite (0 : desactive).
    pub margin: u8,
}

impl Default for IsoSettings {
    fn default() -> Self { IsoSettings { keep: IsoKeep::Folder, file: String::new(), capacity: 0, opts: IsoOptions::default(), verify: true, margin: DEFAULT_MARGIN } }
}

impl IsoSettings {
    pub fn enabled(&self) -> bool { self.keep != IsoKeep::Folder }

    /// Nom du volume effectivement ecrit (titre du disque si vide, tronque a la limite UDF).
    pub fn label(&self, title: &str) -> String { fit_label(if self.opts.label.trim().is_empty() { title } else { &self.opts.label }) }

    pub fn from_value(v: Option<&crate::json::Value>) -> std::result::Result<IsoSettings, String> {
        let mut s = IsoSettings::default();
        let Some(v) = v else { return Ok(s) };
        let st = |k: &str| v.get(k).and_then(crate::json::Value::as_str).unwrap_or("").to_string();
        let flag = |k: &str, d: bool| v.get(k).and_then(crate::json::Value::as_bool).unwrap_or(d);
        s.keep = match v.get("keep").and_then(crate::json::Value::as_str) {
            None | Some("dossier") => IsoKeep::Folder,
            Some("iso") => IsoKeep::Iso,
            Some("les_deux") => IsoKeep::Both,
            Some(x) => return Err(trf!("iso.keep: “{}” unknown (dossier, iso, les_deux)", x)),
        };
        s.file = st("file");
        if let Some(c) = v.get("capacity").and_then(crate::json::Value::as_str) {
            s.capacity = CAPACITIES.iter().position(|(n, _)| *n == c).ok_or(trf!("iso.capacity: “{}” unknown (BD-25, BD-50, BD-100)", c))?;
        }
        s.opts.label = st("volume_name");
        s.opts.owner = st("owner");
        s.opts.organization = st("organization");
        s.opts.contact = st("contact");
        s.opts.udf = match v.get("udf").and_then(crate::json::Value::as_str) {
            None | Some("2.50") => UdfRev::V250,
            Some("2.60") => UdfRev::V260,
            Some(x) => return Err(trf!("iso.udf: “{}” unknown (2.50, 2.60)", x)),
        };
        s.opts.align = flag("align_blocks", true);
        s.opts.play_order = flag("play_order", true);
        s.opts.mirror = flag("mirror", true);
        s.opts.date = match v.get("date").and_then(crate::json::Value::as_str) {
            Some(t) => IsoDate::Custom(IsoStamp::parse(t).ok_or(trf!("iso.date: “{}” invalid (YYYY-MM-DD HH:MM)", t))?),
            None => if flag("fixed_date", false) { IsoDate::Fixed } else { IsoDate::Now },
        };
        s.opts.compat = flag("compatibility", true);
        if let Some(m) = v.get("margin").and_then(crate::json::Value::as_f64) {
            if !(0.0..=100.0).contains(&m) { return Err(trf!("iso.margin: 0 to 100 (0: disabled), {} given", m)); }
            s.margin = m.round() as u8;
        }
        s.verify = flag("verify", true);
        Ok(s)
    }

    /// `None` si tout est par defaut (la cle est alors retiree du projet).
    pub fn to_value(&self) -> Option<crate::json::Value> {
        use crate::json::Value;
        let d = IsoSettings::default();
        let mut o: Vec<(String, Value)> = Vec::new();
        let mut text = |k: &str, v: &str| if !v.is_empty() { o.push((k.into(), Value::Str(v.into()))); };
        text("keep", match self.keep { IsoKeep::Folder => "", IsoKeep::Iso => "iso", IsoKeep::Both => "les_deux" });
        text("file", &self.file);
        text("capacity", if self.capacity == 0 { "" } else { CAPACITIES.get(self.capacity).map_or("", |c| c.0) });
        text("volume_name", &self.opts.label);
        text("owner", &self.opts.owner);
        text("organization", &self.opts.organization);
        text("contact", &self.opts.contact);
        text("udf", if self.opts.udf == UdfRev::V260 { "2.60" } else { "" });
        for (k, v, dv) in [("align_blocks", self.opts.align, d.opts.align), ("play_order", self.opts.play_order, d.opts.play_order), ("mirror", self.opts.mirror, d.opts.mirror), ("fixed_date", self.opts.date == IsoDate::Fixed, false), ("compatibility", self.opts.compat, d.opts.compat), ("verify", self.verify, d.verify)] {
            if v != dv { o.push((k.into(), Value::Bool(v))); }
        }
        if let IsoDate::Custom(c) = self.opts.date { o.push(("date".into(), Value::Str(c.text()))); }
        if self.margin != d.margin { o.push(("margin".into(), Value::Num(self.margin as f64))); }
        if o.is_empty() { None } else { Some(Value::Obj(o)) }
    }

    /// Fichier de l'image : reglage (relatif au dossier du projet), sinon `<dossier construit>.iso`.
    pub fn iso_path(&self, project_dir: &Path, out_dir: &Path) -> PathBuf {
        if !self.file.trim().is_empty() {
            let q = Path::new(self.file.trim());
            return if q.is_absolute() { q.to_path_buf() } else { project_dir.join(q) };
        }
        let name = out_dir.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "disque".into());
        out_dir.with_file_name(format!("{name}.iso"))
    }
}

fn gb(b: u64) -> String { trf!("{} GB", crate::i18n::num(format!("{:.1}", b as f64 / 1e9))) }

/// Avertissement de capacite pour une image de `size` octets : au-dela du disque vise, ou de la marge de securite (`margin` %,
/// 0 : desactivee).
pub fn capacity_warning(size: u64, capacity: usize, margin: u8) -> Option<String> {
    let (cap_name, cap) = CAPACITIES[capacity.min(CAPACITIES.len() - 1)];
    if size > cap {
        Some(trf!("Warning: the image ({}) exceeds the capacity of a {} ({}).", gb(size), cap_name, gb(cap)))
    } else if margin > 0 && size as f64 > cap as f64 * margin as f64 / 100.0 {
        let pct = format!("{:.1}", size as f64 / cap as f64 * 100.0).replace('.', ",");
        Some(trf!("Warning: the image takes up {} % of a {}, beyond the safety margin ({} %): burning is less reliable right at the edge of the disc.", pct, cap_name, margin))
    } else {
        None
    }
}

/// Apres la construction : ecrit l'image du dossier `out_dir` dans `iso`, la verifie si demande, previent si elle depasse la
/// capacite visee, puis supprime le dossier si seule l'image est gardee. Rend un resume pour l'utilisateur.
pub fn finish_build(out_dir: &Path, iso: &Path, s: &IsoSettings, title: &str, cancel: &AtomicBool, log: &mut dyn FnMut(&str), progress: &mut dyn FnMut(f64)) -> Result<String> {
    let opt = IsoOptions { label: s.label(title), ..s.opts.clone() };
    let size = iso_size(out_dir, &opt)?;
    let (cap_name, cap) = CAPACITIES[s.capacity.min(CAPACITIES.len() - 1)];
    let mut summary = String::new();
    let warn = capacity_warning(size, s.capacity, s.margin);
    if let Some(m) = warn {
        log(&m);
        summary += &m;
        summary.push('\n');
    }
    log(&trf!("ISO image: writing {} ({}, volume “{}”)", iso.display(), gb(size), opt.label));
    if let Some(d) = iso.parent() { if !d.as_os_str().is_empty() { std::fs::create_dir_all(d)?; } }
    let verify = s.verify;
    let rep = write_iso(out_dir, iso, &opt, cancel, &mut |p| progress(if verify { p * 0.6 } else { p }))?;
    if verify {
        log(tr("ISO image: verifying…"));
        let n = verify_iso(iso, out_dir, cancel, &mut |p| progress(0.6 + p * 0.4))?;
        log(&trf!("ISO image: verified, {} files identical to the folder", n));
    }
    summary += &trf!("ISO image: {} ({}, {} % of a {}){}", iso.display(), gb(rep.bytes), format!("{:.0}", rep.bytes as f64 / cap as f64 * 100.0), cap_name, if verify { tr(", verified") } else { "" });
    if s.keep == IsoKeep::Iso {
        std::fs::remove_dir_all(out_dir)?;
        log(tr("build folder deleted (only the image is kept)"));
    }
    Ok(summary)
}

/// Dossier de construction temporaire quand seule l'image est gardee : a cote de l'image, vide.
pub fn temp_build_dir(iso: &Path) -> Result<PathBuf> {
    let stem = iso.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "disque".into());
    let d = iso.with_file_name(format!("{stem}.construction"));
    if d.exists() { std::fs::remove_dir_all(&d)?; }
    Ok(d)
}
