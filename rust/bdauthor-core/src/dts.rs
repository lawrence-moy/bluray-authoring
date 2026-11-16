//! Pistes DTS recopiees telles quelles (DTS, DTS-HD High Resolution, DTS-HD Master Audio).
//!
//! Une trame DTS-HD est un coeur DTS (lu par tous les lecteurs) suivi d'une sous-trame d'extension (synchro 0x64582025) qui
//! porte la partie haute definition. Sur un Blu-ray, les deux parties d'une meme trame sont dans la meme piste (meme PID) mais
//! dans deux paquets PES distincts, reconnus par leur identifiant d'extension : 0x71 pour le coeur, 0x72 pour l'extension. Un
//! lecteur sans decodeur DTS-HD joue le coeur. Un DTS simple (sans extension) est ecrit comme l'AC-3, en flux prive 1 (0xBD).
use crate::reader::{err, PResult, ParseError};

/// Types de flux Blu-ray.
pub const STREAM_DTS: u8 = 0x82;
pub const STREAM_DTS_HD_HRA: u8 = 0x85;
pub const STREAM_DTS_HD_MA: u8 = 0x86;
/// Tous les types DTS, y compris 0x8A (DTS hors Blu-ray, parfois ecrit par d'autres outils).
pub const TYPES: [u8; 4] = [STREAM_DTS, STREAM_DTS_HD_HRA, STREAM_DTS_HD_MA, 0x8A];

/// Identifiants d'extension des paquets PES (stream_id 0xFD) : coeur et extension haute definition.
pub const PES_EXT_CORE: u8 = 0x71;
pub const PES_EXT_HD: u8 = 0x72;

const CORE_SYNC: [u8; 4] = [0x7F, 0xFE, 0x80, 0x01];
const EXT_SYNC: [u8; 4] = [0x64, 0x58, 0x20, 0x25];
/// Synchro de l'extension sans perte (XLL) : presente dans le DTS-HD Master Audio.
const XLL_SYNC: [u8; 4] = [0x41, 0xA2, 0x95, 0x47];

/// En-tete d'une trame coeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Core {
    /// taille de la trame coeur (octets)
    pub len: usize,
    /// echantillons par canal
    pub samples: u32,
    /// disposition des canaux (AMODE) : 0 mono, 1 a 4 stereo, 5 et plus multicanal
    pub amode: u8,
    /// frequence (code SFREQ ; 13 = 48 kHz)
    pub sfreq: u8,
}

impl Core {
    /// En-tete du coeur au debut de `f` (DTS 16 bits gros-boutiste, comme dans les fichiers MKV, MP4 et TS).
    pub fn parse(f: &[u8]) -> PResult<Core> {
        if f.len() < 12 || f[..4] != CORE_SYNC {
            return err("synchro DTS absente");
        }
        let b = u64::from_be_bytes([0, 0, f[4], f[5], f[6], f[7], f[8], f[9]]);
        let bits = |off: u32, n: u32| -> u64 { (b >> (48 - off - n)) & ((1 << n) - 1) };
        let len = bits(14, 14) as usize + 1;
        if len < 96 {
            return err("trame DTS trop courte");
        }
        Ok(Core { len, samples: (bits(7, 7) as u32 + 1) * 32, amode: bits(28, 6) as u8, sfreq: bits(34, 4) as u8 })
    }

    pub fn rate(&self) -> Option<u32> {
        match self.sfreq { 13 => Some(48000), 8 => Some(44100), 12 => Some(24000), 3 => Some(32000), _ => None }
    }

    /// Duree de la trame en ticks 90 kHz.
    pub fn ticks(&self) -> u64 { self.samples as u64 * 90_000 / self.rate().unwrap_or(48000) as u64 }
}

/// Separe une trame en coeur et extension haute definition (vide pour un DTS simple).
pub fn split(f: &[u8]) -> PResult<(&[u8], &[u8])> {
    let c = Core::parse(f)?;
    if c.len > f.len() {
        return err("trame DTS tronquee");
    }
    let rest = &f[c.len..];
    if rest.is_empty() {
        return Ok((f, &[]));
    }
    if rest.len() < 4 || rest[..4] != EXT_SYNC {
        return Err(ParseError("donnees inattendues apres le coeur DTS".into()));
    }
    Ok((&f[..c.len], rest))
}

/// Type Blu-ray d'apres les extensions des trames : Master Audio si une extension contient la partie sans perte, High
/// Resolution si une extension existe sans elle, DTS simple sinon.
pub fn stream_type<'a>(exts: impl IntoIterator<Item = &'a [u8]>) -> u8 {
    let mut t = STREAM_DTS;
    for e in exts {
        if e.is_empty() { continue; }
        if e.windows(4).any(|w| w == XLL_SYNC) { return STREAM_DTS_HD_MA; }
        t = STREAM_DTS_HD_HRA;
    }
    t
}

/// Le type d'une piste porte une extension haute definition (deux paquets PES par trame).
pub fn is_hd(stream_type: u8) -> bool { matches!(stream_type, STREAM_DTS_HD_HRA | STREAM_DTS_HD_MA) }

/// Octet d'attributs Blu-ray (presentation << 4 | frequence) d'apres le coeur ; seul 48 kHz est accepte.
pub fn attrs(core: &[u8]) -> PResult<(u8, u8)> {
    let c = Core::parse(core)?;
    if c.rate() != Some(48000) {
        return err("DTS : seul 48 kHz est gere");
    }
    Ok((if c.amode == 0 { 1 } else if c.amode <= 4 { 3 } else { 6 }, 1))
}

/// Descripteurs de la PMT : enregistrement HDMV avec le type et les attributs.
pub fn registration_descriptor(stream_type: u8, core: &[u8]) -> PResult<Vec<u8>> {
    let (p, r) = attrs(core)?;
    Ok(vec![0x05, 0x08, b'H', b'D', b'M', b'V', 0xFF, stream_type, (p << 4) | r, 0xFF])
}

/// Identifiant d'extension d'un paquet PES (`header` : en-tete brut), s'il en a un (stream_id 0xFD).
pub fn pes_extension_id(header: &[u8]) -> Option<u8> {
    if header.len() < 9 || header[3] != 0xFD || header[7] & 0x01 == 0 {
        return None;
    }
    let flags = header[7];
    let mut i = 9;
    if flags & 0x80 != 0 { i += 5; }
    if flags & 0x40 != 0 { i += 5; }
    if flags & 0x20 != 0 { i += 6; }
    if flags & 0x10 != 0 { i += 3; }
    if flags & 0x08 != 0 { i += 1; }
    if flags & 0x04 != 0 { i += 1; }
    if flags & 0x02 != 0 { i += 2; }
    let ext = *header.get(i)?;
    i += 1;
    if ext & 0x80 != 0 { i += 16; }
    if ext & 0x40 != 0 { i += 1 + *header.get(i)? as usize; }
    if ext & 0x20 != 0 { i += 2; }
    if ext & 0x10 != 0 { i += 2; }
    if ext & 0x01 == 0 { return None; }
    // marqueur + longueur, puis drapeau (0) et identifiant sur 7 bits
    let id = *header.get(i + 1)?;
    (id & 0x80 == 0).then_some(id & 0x7F)
}
