//! Audio LPCM de Blu-ray (type de flux 0x80) : trames precedees d'un en-tete de 4 octets, reperage dans un PES et descripteur
//! de la PMT. Les trames sont produites par l'encodeur `pcm_bluray` de ffmpeg ; ici on ne fait que les decouper, les dater et
//! les decrire.
use crate::reader::*;

/// Type de flux MPEG-TS de l'audio LPCM d'un Blu-ray.
pub const STREAM_TYPE: u8 = 0x80;

/// En-tete d'une trame LPCM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// taille des donnees qui suivent l'en-tete (octets)
    pub payload: usize,
    /// affectation des canaux (1 mono, 3 stereo, 4 a 11 : autres dispositions jusqu'au 7.1)
    pub channel_code: u8,
    /// code de frequence (1 : 48 kHz, 4 : 96 kHz, 5 : 192 kHz)
    pub freq_code: u8,
    /// profondeur codee (16, 20 ou 24 bits)
    pub bits: u32,
}

impl Header {
    /// Lit l'en-tete au debut de `d`.
    pub fn parse(d: &[u8]) -> PResult<Header> {
        if d.len() < 4 { return err("trame LPCM tronquee"); }
        let payload = u16::from_be_bytes([d[0], d[1]]) as usize;
        let (channel_code, freq_code) = (d[2] >> 4, d[2] & 0xF);
        let bits = match d[3] >> 6 { 1 => 16, 2 => 20, 3 => 24, _ => return err("profondeur LPCM invalide") };
        if payload == 0 { return err("trame LPCM vide"); }
        Ok(Header { payload, channel_code, freq_code, bits })
    }

    /// Nombre de canaux reels de la disposition.
    pub fn channels(&self) -> u32 { channels(self.channel_code) }

    /// Frequence d'echantillonnage (Hz), 0 si inconnue.
    pub fn rate(&self) -> u32 { match self.freq_code { 1 => 48_000, 4 => 96_000, 5 => 192_000, _ => 0 } }

    /// Octets par echantillon (tous canaux) : les canaux sont ranges par paires, 20 bits compte pour 24.
    pub fn sample_bytes(&self) -> usize { ((self.channels() as usize + 1) / 2 * 2) * if self.bits == 16 { 2 } else { 3 } }

    /// Nombre d'echantillons de la trame.
    pub fn samples(&self) -> usize { self.payload / self.sample_bytes() }

    /// Duree de la trame en ticks de 90 kHz (exact a 48 kHz).
    pub fn ticks(&self) -> u64 { self.samples() as u64 * 90_000 / self.rate().max(1) as u64 }
}

/// Nombre de canaux d'une affectation (mono, stereo, L-C-R, L-R-S, L-C-R-S, L-R-Ls-Rs, 5.0, 5.1, 7.0, 7.1).
pub fn channels(code: u8) -> u32 {
    match code { 1 => 1, 3 => 2, 4 | 5 => 3, 6 | 7 => 4, 8 => 5, 9 => 6, 10 => 7, 11 => 8, _ => 0 }
}

/// Decoupe un bloc de PES en trames : (trame entiere avec son en-tete, duree en ticks). Seul 48 kHz est gere.
pub fn frames(payload: &[u8]) -> PResult<Vec<(&[u8], u64)>> {
    let (mut out, mut pos) = (Vec::new(), 0usize);
    while pos < payload.len() {
        let h = Header::parse(&payload[pos..])?;
        if h.rate() != 48_000 { return err("seul 48 kHz est gere en LPCM"); }
        if h.channels() == 0 { return err("disposition de canaux LPCM inconnue"); }
        let end = pos + 4 + h.payload;
        if end > payload.len() { return err("trame LPCM tronquee"); }
        out.push((&payload[pos..end], h.ticks()));
        pos = end;
    }
    Ok(out)
}

/// (type de presentation, code de frequence) tels que declares dans le fichier CLPI et la playlist : mono 1, stereo 3, autres 6.
pub fn attrs(first_frame: &[u8]) -> PResult<(u8, u8)> {
    let h = Header::parse(first_frame)?;
    Ok((match h.channel_code { 1 => 1, 3 => 3, _ => 6 }, h.freq_code))
}

/// Descripteur d'enregistrement « HDMV » de la PMT pour un flux LPCM : type de codage, presentation et frequence.
pub fn registration_descriptor(first_frame: &[u8]) -> PResult<Vec<u8>> {
    let (pres, rate) = attrs(first_frame)?;
    Ok(vec![0x05, 0x08, b'H', b'D', b'M', b'V', 0xFF, STREAM_TYPE, (pres << 4) | rate, 0xFF])
}
