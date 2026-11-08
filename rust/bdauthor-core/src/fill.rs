//! Debit video qui remplit un disque : ce qui reste de la capacite, une fois retires le menu, l'audio et les videos recopiees,
//! est reparti sur les videos a encoder (meme debit moyen pour toutes, au prorata de leur duree).
use crate::trf;

/// Surcout du multiplexage M2TS (paquets de 192 octets pour 188, en-tetes PES, PCR, tables) sur les flux elementaires.
pub const MUX_OVERHEAD: f64 = 1.035;

/// Place gardee pour les menus, leurs fonds, les sous-titres et les fichiers de navigation : fixe + par video.
pub fn menu_reserve_bytes(videos: usize) -> u64 {
    (200 + 8 * videos as u64) << 20
}

/// Debit en kb/s d'un reglage ffmpeg comme « 192k » ou « 640000 » (192 si illisible).
pub fn parse_kbps(s: &str) -> u32 {
    let t = s.trim().to_ascii_lowercase();
    match t.strip_suffix('k') {
        Some(n) => n.trim().parse().unwrap_or(192),
        None => t.parse::<u32>().map_or(192, |b| (b / 1000).max(1)),
    }
}

/// Taille (octets) de `tracks` pistes audio de `kbps` kb/s pendant `seconds` secondes.
pub fn audio_bytes(kbps: u32, tracks: usize, seconds: f64) -> u64 {
    (kbps as f64 * 1000.0 / 8.0 * seconds.max(0.0) * tracks.max(1) as f64) as u64
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillInput {
    /// capacite du disque (octets)
    pub capacity: u64,
    /// part de la capacite qu'on accepte de remplir (%), comme la marge de securite de l'image ISO ; 0 : 97
    pub margin_pct: u8,
    /// place gardee pour les menus et fichiers annexes (octets)
    pub reserve: u64,
    /// taille de toutes les pistes audio (octets, flux elementaires)
    pub audio_bytes: u64,
    /// taille des flux video recopies sans reencodage (octets)
    pub copied_bytes: u64,
    /// duree totale des videos a encoder (s)
    pub encode_seconds: f64,
    /// debit maximal moyen qu'on s'autorise (kb/s) : inutile de depasser ce qu'un disque vide de contenu n'exige pas
    pub cap_kbps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillPlan {
    /// debit moyen a viser (kb/s)
    pub kbps: u32,
    /// debit que la place permettrait (kb/s), avant plafonnement
    pub room_kbps: f64,
    /// octets utilisables au total
    pub usable: u64,
}

/// Debit video moyen qui remplit le disque ; erreur si le contenu ne tient pas (moins de 800 kb/s de video).
pub fn fill_bitrate(i: &FillInput) -> Result<FillPlan, String> {
    let pct = if i.margin_pct == 0 { 97 } else { i.margin_pct.min(100) } as u64;
    let usable = (i.capacity as u128 * pct as u128 / 100) as u64;
    let fixed = i.reserve as f64 + (i.audio_bytes + i.copied_bytes) as f64 * MUX_OVERHEAD;
    let avail = usable as f64 - fixed;
    if i.encode_seconds <= 0.0 {
        return Ok(FillPlan { kbps: i.cap_kbps, room_kbps: f64::INFINITY, usable });
    }
    let room = if avail > 0.0 { avail / MUX_OVERHEAD * 8.0 / i.encode_seconds / 1000.0 } else { 0.0 };
    if room < 800.0 {
        return Err(trf!("the content does not fit: only {} kb/s would be left for the video (at least 800 kb/s needed); use a larger disc or shorten the videos", format!("{:.0}", room.max(0.0))));
    }
    Ok(FillPlan { kbps: (room.min(i.cap_kbps as f64)).floor() as u32, room_kbps: room, usable })
}

/// Phrase de compte rendu pour le journal.
pub fn describe(p: &FillPlan, disc: &str) -> String {
    if p.room_kbps.is_finite() && p.room_kbps > p.kbps as f64 + 1.0 {
        trf!("target video bitrate: {} kb/s (the {} would allow {} kb/s: capped)", p.kbps, disc, format!("{:.0}", p.room_kbps))
    } else {
        trf!("target video bitrate: {} kb/s to fill the {}", p.kbps, disc)
    }
}
