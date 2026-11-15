//! Occupation estimee du disque, calculee pendant l'edition (sans encoder) : videos, audio, sous-titres, menus et multiplexage,
//! compares a la capacite du disque vise (reglages de l'image ISO).
//!
//! Les tailles audio sont exactes a la duree pres (debits fixes). La taille des videos encodees avec un CRF ne se connait qu'apres
//! l'encodage : elle est estimee d'apres le CRF et la definition, sans depasser de beaucoup le debit de la source ni le debit
//! maximal de l'encodage. Avec « remplir le disque », les videos prennent la place restante.
use crate::audio::AudioFormat;
use crate::fill::{audio_bytes, menu_reserve_bytes, MUX_OVERHEAD};
use crate::i18n::tr;
use crate::model::ProjectModel;

/// Ce que l'interface sait d'une video (`None` : pas encore lu).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VideoFacts {
    pub duration: Option<f64>,
    /// hauteur de l'image (pixels)
    pub height: Option<u32>,
    /// debit moyen du fichier source, tous flux compris (kb/s)
    pub source_kbps: Option<u32>,
}

/// Une part de l'occupation.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub label: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    /// videos, audio, sous-titres, menus, multiplexage
    pub parts: Vec<Part>,
    pub total: u64,
    /// nom et capacite du disque vise
    pub disc: &'static str,
    pub capacity: u64,
    /// part de la capacite qu'on accepte de remplir (%)
    pub margin_pct: u8,
    /// « remplir le disque » : les videos prennent la place restante
    pub fill: bool,
    /// videos dont la duree n'est pas encore connue (non comptees)
    pub unknown: usize,
}

impl Usage {
    pub fn usable(&self) -> u64 { (self.capacity as u128 * self.margin_pct as u128 / 100) as u64 }
    pub fn ratio(&self) -> f64 { self.total as f64 / self.capacity.max(1) as f64 }
    /// Au-dela de la marge de securite.
    pub fn over_margin(&self) -> bool { self.total > self.usable() }
    pub fn over_capacity(&self) -> bool { self.total > self.capacity }
}

/// Debit video estime (kb/s) d'un encodage x264 au CRF `crf` d'une image de `height` lignes : environ 9 Mb/s en 1080p a CRF 18,
/// divise par deux tous les 6 points de CRF, proportionnel au nombre de pixels.
pub fn crf_kbps(crf: u32, height: u32) -> f64 {
    let px = (height.max(240) as f64 / 1080.0).powi(2);
    9000.0 * 2f64.powf((18.0 - crf as f64) / 6.0) * px
}

/// Debit audio par defaut des pistes Dolby Digital (kb/s).
pub const AC3_DEFAULT_KBPS: u32 = 192;
/// Debit maximal de la video encodee (kb/s), comme `EncodeOpts::default().maxrate`.
pub const MAXRATE_KBPS: f64 = 30000.0;
/// Taille estimee d'une piste de sous-titres (octets par heure de video).
pub const SUBS_BYTES_PER_HOUR: f64 = 25.0 * 1024.0 * 1024.0;

/// Occupation estimee du disque.
pub fn estimate(m: &ProjectModel, facts: &dyn Fn(usize) -> VideoFacts) -> Usage {
    let (disc, capacity) = crate::iso::CAPACITIES[m.iso.capacity.min(crate::iso::CAPACITIES.len() - 1)];
    let margin_pct = if m.iso.margin == 0 { 97 } else { m.iso.margin.min(100) };
    let n = m.videos.len();
    let (mut video, mut audio, mut subs) = (0f64, 0u64, 0f64);
    let mut unknown = 0;
    for (i, v) in m.videos.iter().enumerate() {
        let f = facts(i);
        let Some(d) = f.duration.filter(|d| *d > 0.0) else { unknown += 1; continue };
        // audio : debits fixes ; canaux inconnus ici, LPCM compte en 5.1 (estimation prudente)
        if v.audio.is_empty() {
            audio += audio_bytes(AC3_DEFAULT_KBPS, 1, d);
        } else {
            for t in &v.audio {
                let ch = if t.format.is_lpcm() { 6 } else { 2 };
                let src = if t.format == AudioFormat::Ac3Copy { 640 } else { 0 };
                audio += audio_bytes(t.kbps(ch, src, AC3_DEFAULT_KBPS), 1, d);
            }
        }
        subs += v.subs.len() as f64 * SUBS_BYTES_PER_HOUR * d / 3600.0;
        if !m.fill_disc {
            let mut kbps = crf_kbps(m.crf, f.height.unwrap_or(1080)).min(MAXRATE_KBPS);
            // reencoder une source peu compressee n'en fait pas un gros fichier : pas beaucoup plus que la source
            if let Some(s) = f.source_kbps.filter(|s| *s > 0) { kbps = kbps.min((s as f64 * 1.3).max(1500.0)); }
            video += kbps * 1000.0 / 8.0 * d;
        }
    }
    let menus = menu_reserve_bytes(n);
    let usable = (capacity as u128 * margin_pct as u128 / 100) as u64;
    if m.fill_disc {
        // les videos a encoder prennent ce qui reste sous la marge
        let fixed = menus as f64 + audio as f64 * MUX_OVERHEAD + subs;
        video = ((usable as f64 - fixed) / MUX_OVERHEAD).max(0.0);
    }
    let mux = ((video + audio as f64) * (MUX_OVERHEAD - 1.0)) as u64;
    let parts = vec![
        Part { label: tr("Videos").to_string(), bytes: video as u64 },
        Part { label: tr("Audio").to_string(), bytes: audio },
        Part { label: tr("Subtitles").to_string(), bytes: subs as u64 },
        Part { label: tr("Menus and navigation").to_string(), bytes: menus },
        Part { label: tr("Multiplexing").to_string(), bytes: mux },
    ];
    let total = parts.iter().map(|p| p.bytes).sum();
    Usage { parts, total, disc, capacity, margin_pct, fill: m.fill_disc, unknown }
}

/// Taille lisible en Go (1 Go = 10^9 octets, comme les capacites des disques).
pub fn gb(bytes: u64) -> String { format!("{:.1}", bytes as f64 / 1e9) }
