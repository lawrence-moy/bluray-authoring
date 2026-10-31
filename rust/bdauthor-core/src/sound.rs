//! Sons des menus (fichier BDMV/AUXDATA/sound.bdmv) : sons courts joues par le lecteur quand la selection change ou qu'un bouton
//! est active. Format d'apres libbluray (`sound_parse.c`) : en-tete « BCLK0200 », table des sons, puis echantillons LPCM 48 kHz
//! 16 bits gros-boutiste, entrelaces en stereo.

/// Un son : echantillons 16 bits gros-boutistes (entrelaces si stereo), 48 kHz.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    /// 1 (mono) ou 2 (stereo)
    pub channels: u8,
    pub samples: Vec<u8>,
}

impl Sound {
    /// Duree en secondes.
    pub fn seconds(&self) -> f64 { self.samples.len() as f64 / (2.0 * self.channels.max(1) as f64 * 48_000.0) }
}

/// Nombre maximal de sons (identifiant sur un octet, 0xFF = aucun son).
pub const MAX_SOUNDS: usize = 255;

/// Contenu de sound.bdmv. Les identifiants de son des boutons sont les rangs dans `sounds`.
pub fn make_sound_bdmv(sounds: &[Sound]) -> Vec<u8> {
    let n = sounds.len().min(MAX_SOUNDS);
    let index_len = 2 + 10 * n as u32; // reserve + nombre de sons + entrees
    let data_start = 40 + 4 + index_len;
    let mut o = Vec::new();
    o.extend_from_slice(b"BCLK0200");
    o.extend_from_slice(&data_start.to_be_bytes());
    o.extend_from_slice(&0u32.to_be_bytes()); // pas de donnees d'extension
    o.resize(40, 0); // reserve
    o.extend_from_slice(&index_len.to_be_bytes());
    o.push(0); // reserve
    o.push(n as u8);
    let mut offset = 0u32;
    for s in &sounds[..n] {
        // attributs : canaux (1 mono, 3 stereo) sur 4 bits, frequence (1 = 48 kHz) sur 4 bits ; 16 bits (1) sur 2 bits + 6 bits de bourrage
        o.push((if s.channels >= 2 { 3 } else { 1 }) << 4 | 1);
        o.push(1 << 6);
        o.extend_from_slice(&offset.to_be_bytes());
        o.extend_from_slice(&(s.samples.len() as u32).to_be_bytes());
        offset += s.samples.len() as u32;
    }
    debug_assert_eq!(o.len() as u32, data_start);
    for s in &sounds[..n] { o.extend_from_slice(&s.samples); }
    o
}

/// Relecture (verification) : (canaux, frequence, bits, position des echantillons dans le fichier, longueur en octets) de chaque son.
pub fn parse_sound_bdmv(d: &[u8]) -> Result<Vec<(u8, u32, u8, usize, usize)>, String> {
    let u32_at = |p: usize| -> Result<u32, String> { d.get(p..p + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| "sound.bdmv tronque".to_string()) };
    if d.get(0..8) != Some(b"BCLK0200".as_slice()) && d.get(0..8) != Some(b"BCLK0100".as_slice()) { return Err("sound.bdmv : signature inconnue".into()); }
    let data_start = u32_at(8)? as usize;
    let n = *d.get(45).ok_or("sound.bdmv tronque")? as usize;
    let mut out = Vec::new();
    for k in 0..n {
        let e = 46 + 10 * k;
        let (a, b) = (*d.get(e).ok_or("sound.bdmv tronque")?, *d.get(e + 1).ok_or("sound.bdmv tronque")?);
        let channels = match a >> 4 { 1 => 1, 3 => 2, c => return Err(format!("son {k} : canaux {c} inconnus")) };
        let rate = match a & 0x0f { 1 => 48_000, r => return Err(format!("son {k} : frequence {r} inconnue")) };
        let bits = match b >> 6 { 1 => 16, x => return Err(format!("son {k} : format {x} inconnu")) };
        let (off, len) = (u32_at(e + 2)? as usize, u32_at(e + 6)? as usize);
        if data_start + off + len > d.len() { return Err(format!("son {k} : echantillons hors du fichier")); }
        out.push((channels, rate, bits, data_start + off, len));
    }
    Ok(out)
}
