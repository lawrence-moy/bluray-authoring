//! Pistes Dolby TrueHD recopiees telles quelles, avec leur coeur AC-3.
//!
//! Sur un Blu-ray, une piste TrueHD (type de flux 0x83) porte deux flux dans le meme PID, en paquets PES distincts reconnus par leur
//! identifiant d'extension (stream_id 0xFD) : 0x76 pour un coeur AC-3 (lu par tous les lecteurs) et 0x72 pour le TrueHD. Les
//! sources (MKV) n'ont presque jamais ce coeur : il est encode a part (AC-3 640 kb/s au plus) puis entrelace ici avec les unites
//! d'acces TrueHD, regroupees par trame AC-3 (voir `group_of`).
//!
//! Non verifie sur un lecteur de salon : le decoupage des unites en paquets (un paquet TrueHD par trame AC-3, coupe a
//! `MAX_PES_BYTES` octets) et les attributs de clip sont ceux que l'on deduit de la norme et des outils existants.
use crate::reader::{err, PResult, ParseError};

/// Type de flux Blu-ray du TrueHD (avec son coeur AC-3).
pub const STREAM_TYPE: u8 = 0x83;
/// Identifiants d'extension des paquets PES : coeur AC-3 et TrueHD.
pub const PES_EXT_CORE: u8 = 0x76;
pub const PES_EXT_TRUEHD: u8 = 0x72;
/// Debit suppose d'une piste TrueHD recopiee dont le debit est inconnu (kb/s ; un TrueHD 5.1 tourne autour de 3 a 5 Mb/s).
pub const TRUEHD_KBPS: u32 = 5000;
/// Taille maximale des donnees TrueHD d'un paquet PES (la longueur d'un PES audio tient sur 16 bits).
pub const MAX_PES_BYTES: usize = 60_000;

/// Synchro principale d'un flux TrueHD (0xF8726FBB pour le MLP du DVD-Audio).
const MAJOR_SYNC: [u8; 4] = [0xF8, 0x72, 0x6F, 0xBA];

/// Une unite d'acces : ses octets (en-tete de 4 octets compris) et sa duree en echantillons par canal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit<'a> {
    pub data: &'a [u8],
    pub samples: u32,
}

/// Frequence d'echantillonnage (Hz) d'apres le code de la synchro principale ; seuls 48 et 96 kHz existent sur Blu-ray.
fn rate_of(ratebits: u8) -> Option<u32> {
    match ratebits { 0 => Some(48_000), 1 => Some(96_000), _ => None }
}

/// Coupe `buf` (des unites d'acces TrueHD bout a bout) en unites. `rate` : frequence connue (0 tant qu'aucune synchro principale n'a
/// ete vue ; elle est mise a jour, pour enchainer plusieurs tampons). Les unites qui precedent la premiere synchro principale ne
/// sont pas decodables : elles sont ignorees.
pub fn units<'a>(buf: &'a [u8], rate: &mut u32) -> PResult<Vec<Unit<'a>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if buf.len() - i < 4 { return err("unite TrueHD tronquee"); }
        let len = ((buf[i] as usize & 0x0F) << 8 | buf[i + 1] as usize) * 2;
        if len < 4 || i + len > buf.len() { return Err(ParseError(format!("unite TrueHD de {len} octets incoherente"))); }
        let au = &buf[i..i + len];
        if au.len() >= 9 && au[4..8] == MAJOR_SYNC {
            *rate = rate_of(au[8] >> 4).ok_or_else(|| ParseError("TrueHD : seuls 48 et 96 kHz sont geres".into()))?;
        }
        if *rate != 0 {
            out.push(Unit { data: au, samples: 40 * (*rate / 48_000) });
        }
        i += len;
    }
    Ok(out)
}

/// Pour chaque unite (instants `au_pts`, croissants), rang de la trame AC-3 (instants `core_pts`, croissants) qui la porte : la
/// derniere trame qui commence au plus `tolerance` ticks apres l'unite ; les unites qui precedent la premiere trame vont a la premiere.
pub fn group_of(core_pts: &[u64], au_pts: &[u64], tolerance: u64) -> Vec<usize> {
    let mut j = 0;
    au_pts.iter().map(|&p| {
        while j + 1 < core_pts.len() && core_pts[j + 1] <= p + tolerance { j += 1; }
        j
    }).collect()
}

/// Octet d'attributs Blu-ray (presentation << 4 | frequence) d'apres le coeur AC-3 : le coeur est toujours a 48 kHz.
pub fn attrs(core: &[u8]) -> PResult<(u8, u8)> { crate::tsclip::ac3_attrs(core) }

/// Descripteurs de la PMT : enregistrement HDMV avec le type et les attributs.
pub fn registration_descriptor(core: &[u8]) -> PResult<Vec<u8>> {
    let (p, r) = attrs(core)?;
    Ok(vec![0x05, 0x08, b'H', b'D', b'M', b'V', 0xFF, STREAM_TYPE, (p << 4) | r, 0xFF])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unite factice : longueur en mots de 16 bits dans les 12 bits bas, puis remplissage.
    fn au(words: usize, major: Option<u8>) -> Vec<u8> {
        let mut v = vec![0u8; words * 2];
        v[0] = (words >> 8) as u8 & 0x0F;
        v[1] = words as u8;
        if let Some(r) = major { v[4..8].copy_from_slice(&MAJOR_SYNC); v[8] = r << 4; }
        v
    }

    #[test]
    fn units_follow_the_sync_rate() {
        let mut buf = au(10, Some(0));
        buf.extend(au(7, None));
        buf.extend(au(9, None));
        let mut rate = 0;
        let u = units(&buf, &mut rate).unwrap();
        assert_eq!(rate, 48_000);
        assert_eq!(u.iter().map(|x| (x.data.len(), x.samples)).collect::<Vec<_>>(), vec![(20, 40), (14, 40), (18, 40)]);
        let mut rate96 = 0;
        assert!(units(&au(10, Some(1)), &mut rate96).unwrap().iter().all(|x| x.samples == 80));
    }

    #[test]
    fn units_before_the_first_sync_are_skipped_and_the_rate_carries_over() {
        let mut buf = au(5, None);
        buf.extend(au(10, Some(0)));
        let mut rate = 0;
        assert_eq!(units(&buf, &mut rate).unwrap().len(), 1);
        assert_eq!(units(&au(6, None), &mut rate).unwrap().len(), 1);
    }

    #[test]
    fn bad_lengths_are_refused() {
        let mut rate = 48_000;
        assert!(units(&[0x00, 0x20, 0, 0], &mut rate).is_err());
        assert!(units(&[0x00, 0x01, 0, 0], &mut rate).is_err());
        assert!(units(&[0, 1], &mut rate).is_err());
        assert!(units(&au(10, Some(3)), &mut 0).is_err());
    }

    #[test]
    fn units_are_grouped_by_core_frame() {
        let core = [1000, 3880, 6760];
        let au: Vec<u64> = (0..10).map(|k| 1000 + k * 833).collect();
        // les unites 0..=3 commencent avant 3880 ; la 4e (1000 + 3 * 833 = 3499) est a moins de 417 ticks de la 2e trame
        let g = group_of(&core, &au, 417);
        assert_eq!(g, vec![0, 0, 0, 1, 1, 1, 1, 2, 2, 2]);
        // avant la premiere trame : premiere trame
        assert_eq!(group_of(&core, &[10, 20_000], 0), vec![0, 2]);
        assert!(group_of(&[], &[5], 0).iter().all(|g| *g == 0));
    }
}
