//! Multiplexeur M2TS d'un film H.264 + AC3 — equivalent de `vmux.py` (memes regles, memes operations flottantes dans le meme ordre,
//! pour produire exactement les memes octets).
use crate::mux::*;

pub const STREAM_ID_VIDEO: u8 = 0xE0;
pub const STREAM_ID_AUDIO: u8 = 0xBD;
pub const AC3_FRAME_TICKS: u64 = 2880; // 1536 echantillons a 48 kHz en ticks de 90 kHz
pub const AC3_WORDS_48K: [u16; 38] = [64, 64, 80, 80, 96, 96, 112, 112, 128, 128, 160, 160, 192, 192, 224, 224, 256, 256, 320, 320,
                                      384, 384, 448, 448, 512, 512, 640, 640, 768, 768, 896, 896, 1024, 1024, 1152, 1152, 1280, 1280];

#[derive(Debug, Clone, PartialEq)]
pub struct VideoAu {
    pub pts: u64,
    pub dts: Option<u64>,
    pub data: Vec<u8>, // unite d'acces, Annex B
}
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    pub pts: u64,
    pub data: Vec<u8>,
}

pub fn build_pes(stream_id: u8, pts: u64, dts: Option<u64>, data: &[u8]) -> Result<Vec<u8>, String> {
    let (flags, ts): (u8, Vec<u8>) = match dts {
        Some(d) if d != pts => (0xC0, [ts5(0b0011, pts), ts5(0b0001, d)].concat()),
        _ => (0x80, ts5(0b0010, pts).to_vec()),
    };
    let mut length = 3 + ts.len() + data.len();
    if length > 0xFFFF {
        if stream_id != STREAM_ID_VIDEO {
            return Err("PES trop long".into());
        }
        length = 0;
    }
    let mut o = vec![0, 0, 1, stream_id];
    o.extend_from_slice(&(length as u16).to_be_bytes());
    o.extend_from_slice(&[0x84, flags, ts.len() as u8]);
    o.extend(ts);
    o.extend_from_slice(data);
    Ok(o)
}

/// Decoupe un PES en paquets TS ; retourne (paquets, compteur de continuite suivant).
pub fn packetize(pes: &[u8], pid: u16, mut cc: u8) -> (Vec<Vec<u8>>, u8) {
    let mut out = Vec::new();
    let (mut pos, mut first) = (0usize, true);
    while pos < pes.len() {
        let chunk = &pes[pos..(pos + PAYLOAD).min(pes.len())];
        pos += chunk.len();
        out.push(pes_packet(pid, first, cc, chunk));
        first = false;
        cc = (cc + 1) & 15;
    }
    (out, cc)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Audio,
    Video,
    Sub,
}

struct Item {
    kind: Kind,
    deadline: u64, // heure de decodage, ticks 27 MHz
    packets: Vec<Vec<u8>>,
    size: usize,
}

enum Slot {
    Pcr,
    Psi(usize, u8), // (indice de section PAT/PMT/SIT, compteur de continuite)
    Pkt(usize, usize),
}

struct Sched {
    out: Vec<(f64, Slot)>,
    late: f64,
    finish: Vec<f64>,
}

fn schedule(items: &[Item], dt: f64, lead: f64, pcr_i: f64, psi_i: f64, margin: f64) -> Sched {
    let mut out: Vec<(f64, Slot)> = Vec::new();
    let mut t = items[0].deadline as f64 - lead - 4.0 * dt;
    let (mut psi_t, mut pcr_t): (f64, f64);
    let mut cc = [0u8; 3];
    macro_rules! psi {
        () => {{
            psi_t = t;
            for k in 0..3 {
                out.push((t, Slot::Psi(k, cc[k])));
                cc[k] = (cc[k] + 1) & 15;
                t += dt;
            }
        }};
    }
    macro_rules! pcr {
        () => {{
            pcr_t = t;
            out.push((t, Slot::Pcr));
            t += dt;
        }};
    }
    psi!();
    pcr!();
    let mut late = f64::NEG_INFINITY;
    let mut finish = Vec::with_capacity(items.len());
    for (ii, it) in items.iter().enumerate() {
        let want = it.deadline as f64 - lead;
        while t < want {
            let (due_pcr, due_psi) = (pcr_t + pcr_i, psi_t + psi_i);
            let due = due_pcr.min(due_psi);
            if due >= want {
                t = want;
                break;
            }
            t = t.max(due);
            if due_pcr <= due_psi { pcr!() } else { psi!() }
        }
        if t - psi_t >= psi_i {
            psi!();
        }
        for k in 0..it.packets.len() {
            if t - pcr_t >= pcr_i {
                pcr!();
            }
            out.push((t, Slot::Pkt(ii, k)));
            t += dt;
        }
        finish.push(t);
        late = late.max(t - it.deadline as f64 + margin);
    }
    Sched { out, late, finish }
}

/// Debit total (video + audio, kb/s) qu'un clip peut porter avec les parametres par defaut du multiplexage (`peak_rate`), avec une
/// marge de 5 % pour les paquets de service (PCR, tables).
pub const MUX_BUDGET_KBPS: u32 = 33_700;

#[derive(Debug, Clone, Copy)]
pub struct MuxParams {
    pub video_pid: u16,
    pub audio_pid: u16,
    pub pmt_pid: u16,
    pub pcr_pid: u16,
    pub peak_rate: u32,
    pub lead_s: f64,
    pub pcr_interval_s: f64,
    pub psi_interval_s: f64,
    pub margin_s: f64,
}
impl Default for MuxParams {
    fn default() -> Self {
        MuxParams { video_pid: 0x1011, audio_pid: 0x1100, pmt_pid: 0x0100, pcr_pid: 0x1001, peak_rate: 88750, lead_s: 0.483,
                    pcr_interval_s: 0.09, psi_interval_s: 0.25, margin_s: 0.002 }
    }
}

#[derive(Debug, Clone)]
pub struct MuxStats {
    pub paquets: usize,
    pub octets: usize,
    pub avance_s: f64,
    pub debit_pic_mbps: f64,
    pub ecart_pcr_max_ms: f64,
    pub tampon_video_max_octets: i64,
    pub pcr: usize,
}

/// Piste de sous-titres PGS a multiplexer : PID et PES (dans l'ordre du flux), horodatages en 90 kHz.
#[derive(Debug, Clone)]
pub struct SubTrack {
    pub pid: u16,
    pub specs: Vec<PesSpec>,
}

/// `video` : unites d'acces en ordre de decodage ; `audio` : trames AC3 ; temps en 90 kHz. `pmt_streams` : flux de la PMT.
pub fn mux_film(video: &[VideoAu], audio: &[AudioFrame], pmt_streams: &[PmtStream], p: &MuxParams) -> Result<(Vec<u8>, MuxStats), String> {
    mux_film_subs(video, audio, &[], pmt_streams, p)
}

/// Comme `mux_film`, avec des pistes de sous-titres PGS (sans elles, la sortie est identique octet pour octet).
pub fn mux_film_subs(
    video: &[VideoAu], audio: &[AudioFrame], subs: &[SubTrack], pmt_streams: &[PmtStream], p: &MuxParams,
) -> Result<(Vec<u8>, MuxStats), String> {
    mux_film_audios(video, &[(p.audio_pid, audio)], subs, pmt_streams, p)
}

/// Comme `mux_film_subs`, avec plusieurs pistes audio : (PID, trames AC3) ; avec une seule piste, la sortie est la meme.
pub fn mux_film_audios(
    video: &[VideoAu], audios: &[(u16, &[AudioFrame])], subs: &[SubTrack], pmt_streams: &[PmtStream], p: &MuxParams,
) -> Result<(Vec<u8>, MuxStats), String> {
    if video.is_empty() {
        return Err("aucune image video".into());
    }
    let rate = p.peak_rate as u64 * 50;
    let dt = (188u64 * 27_000_000) as f64 / rate as f64;
    // (heure de decodage, genre, pid, PES, taille)
    let mut raw: Vec<(u64, Kind, u16, Vec<u8>, usize)> = Vec::with_capacity(video.len() + audios.iter().map(|a| a.1.len()).sum::<usize>());
    let sub_total: usize = subs.iter().map(|t| t.specs.len()).sum();
    raw.reserve(sub_total);
    for v in video {
        raw.push((v.dts.unwrap_or(v.pts) * 300, Kind::Video, p.video_pid, build_pes(STREAM_ID_VIDEO, v.pts, v.dts, &v.data)?, v.data.len()));
    }
    for (apid, audio) in audios {
        for a in audio.iter() {
            raw.push((a.pts * 300, Kind::Audio, *apid, build_pes(STREAM_ID_AUDIO, a.pts, None, &a.data)?, a.data.len()));
        }
    }
    for t in subs {
        for sp in &t.specs {
            raw.push((sp.dts.unwrap_or(sp.pts) * 300, Kind::Sub, t.pid, crate::pgs::build_pg_pes(sp)?, 0));
        }
    }
    raw.sort_by_key(|x| (x.0, x.1 == Kind::Video)); // « a » avant « v », tri stable
    let mut ccv = 0u8;
    let mut ccs: std::collections::HashMap<u16, u8> = std::collections::HashMap::new();
    let items: Vec<Item> = raw
        .into_iter()
        .map(|(dl, kind, pid, pes, size)| {
            // compteur de continuite propre a chaque PID (une seule piste audio : meme sortie qu'avec un compteur unique)
            let cc = if pid == p.video_pid { &mut ccv } else { ccs.entry(pid).or_insert(0) };
            let (packets, next) = packetize(&pes, pid, *cc);
            *cc = next;
            Item { kind, deadline: dl, packets, size }
        })
        .collect();
    let sections = [make_pat(p.pmt_pid), make_pmt(pmt_streams, p.pcr_pid), make_sit(p.peak_rate)];
    let (mut lead, margin) = (p.lead_s * 27_000_000.0, p.margin_s * 27_000_000.0);
    let mut sched = None;
    for _ in 0..40 {
        let s = schedule(&items, dt, lead, p.pcr_interval_s * 27e6, p.psi_interval_s * 27e6, margin);
        if s.late <= 0.0 {
            sched = Some(s);
            break;
        }
        lead += s.late + 27_000.0;
    }
    let Sched { mut out, finish, .. } = sched.ok_or("debit d'arrivee insuffisant pour tenir les heures de decodage")?;
    // tampon video : octets arrives et pas encore decodes, estimes aux heures de decodage
    let mut ev: Vec<(f64, i64)> = Vec::new();
    for (it, &f) in items.iter().zip(&finish) {
        if it.kind == Kind::Video {
            ev.push((f, it.size as i64));
        }
    }
    for it in &items {
        if it.kind == Kind::Video {
            ev.push((it.deadline as f64, -(it.size as i64)));
        }
    }
    ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then((-a.1).cmp(&(-b.1))));
    let (mut occ, mut peak) = (0i64, 0i64);
    for (_, d) in &ev {
        occ += d;
        peak = peak.max(occ);
    }
    let mut t_end = out.last().unwrap().0 + dt;
    let mut n_null = 0usize;
    while (out.len() + n_null) % 32 != 0 {
        n_null += 1;
    }
    let total = out.len() + n_null;
    let mut buf = Vec::with_capacity(total * 192);
    let mut pcr_times: Vec<i64> = Vec::new();
    let emit = |buf: &mut Vec<u8>, t: f64, pkt: Vec<u8>| {
        let ats = (t as i64) & 0x3FFF_FFFF;
        buf.extend_from_slice(&(ats as u32).to_be_bytes());
        buf.extend(pkt);
    };
    let pids = [0u16, p.pmt_pid, 0x1F];
    for (t, slot) in out.drain(..) {
        let pkt = match slot {
            Slot::Pcr => {
                let ti = t as i64;
                pcr_times.push(ti);
                pcr_packet(ti as u64)
            }
            Slot::Psi(k, cc) => psi_packet(pids[k], &sections[k], cc),
            Slot::Pkt(i, k) => items[i].packets[k].clone(),
        };
        emit(&mut buf, t, pkt);
    }
    for _ in 0..n_null {
        emit(&mut buf, t_end, null_packet());
        t_end += dt;
    }
    let stats = MuxStats {
        paquets: total, octets: buf.len(), avance_s: lead / 27e6, debit_pic_mbps: rate as f64 * 8.0 / 1e6,
        ecart_pcr_max_ms: pcr_times.windows(2).map(|w| (w[1] - w[0]) as f64 / 27000.0).fold(0.0, f64::max),
        tampon_video_max_octets: peak, pcr: pcr_times.len(),
    };
    Ok((buf, stats))
}
