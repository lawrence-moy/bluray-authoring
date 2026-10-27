"""Analyse d'un clip .m2ts de film (H.264 + audio AC3) et generation de son .clpi.

Regles retrouvees sur le disque de reference (00000.m2ts / 00000.clpi) :
 - EP_map : un point d'acces par PES video contenant un SPS (NAL type 7) ; PTS = PTS du PES, SPN = paquet de debut ;
   une entree coarse a chaque changement de PTS[32:19] ou de SPN[31:17] ;
 - I_end_position_offset : ceil(log2(taille de l'unite d'acces / 64 Kio)) -- hypothese ajustee sur 19 images
   (valeurs 2 et 3 seulement observees), NON verifiee hors de cette plage ;
 - sequence : debut = plus petit PTS (45 kHz = PTS/2), fin = plus grand PTS video / 2 (PTS de la derniere image, sans
   ajouter sa duree : c'est ce que donne le disque de reference, a un cas pres non distinguable) ;
 - debit : floor(taille du fichier / duree de la sequence).
"""
import math
import statistics
from dataclasses import dataclass
from .m2ts import iter_packets, reassemble
from .clpi import (Clpi, ClipInfo, SequenceInfo, AtcSeq, StcSeq, ProgramInfo, Program, Stream,
                   Cpi, EpStream, EpCoarse, EpFine)

VIDEO_TYPES = {0x1B: "H.264", 0x02: "MPEG-2", 0xEA: "VC-1"}
AUDIO_TYPES = {0x81: "AC3"}
# (duree d'image en ticks 90 kHz, code de cadence du .clpi)
FRAME_RATES = {23.976: (3753.75, 1), 24.0: (3750.0, 2), 25.0: (3600.0, 3), 29.97: (3003.0, 4),
               50.0: (1800.0, 6), 59.94: (1501.5, 7)}


def parse_psi(data: bytes):
    """Retourne (pcr_pid, pmt_pid, [(stream_type, pid), ...]) depuis la PAT et la PMT du flux."""
    def section(pid):
        for p in iter_packets(data):
            if p.pid == pid and p.pusi:
                pl = p.payload
                return pl[1 + pl[0]:]
        raise ValueError(f"section PSI absente (PID {pid:#x})")
    pat = section(0)
    n = ((pat[1] & 0xF) << 8) | pat[2]
    pmt_pid = next(((pat[i + 2] & 0x1F) << 8) | pat[i + 3] for i in range(8, 3 + n - 4, 4)
                   if (pat[i] << 8 | pat[i + 1]) != 0)
    pmt = section(pmt_pid)
    n = ((pmt[1] & 0xF) << 8) | pmt[2]
    pcr_pid = ((pmt[8] & 0x1F) << 8) | pmt[9]
    pos = 12 + (((pmt[10] & 0xF) << 8) | pmt[11])
    streams = []
    while pos < 3 + n - 4:
        streams.append((pmt[pos], ((pmt[pos + 1] & 0x1F) << 8) | pmt[pos + 2]))
        pos += 5 + (((pmt[pos + 3] & 0xF) << 8) | pmt[pos + 4])
    return pcr_pid, pmt_pid, streams


# ---------------------------------------------------------------- H.264
class _Bits:
    def __init__(self, b):
        self.v, self.n, self.p = int.from_bytes(b, "big"), len(b) * 8, 0

    def u(self, k):
        self.p += k
        return (self.v >> (self.n - self.p)) & ((1 << k) - 1)

    def ue(self):
        z = 0
        while self.u(1) == 0:
            z += 1
        return (1 << z) - 1 + (self.u(z) if z else 0)

    def se(self):
        k = self.ue()
        return (k + 1) // 2 if k & 1 else -(k // 2)


def nal_units(b: bytes):
    """[(type, offset_debut_charge, longueur_charge)] pour un flux Annex B."""
    idx, i = [], 0
    while (j := b.find(b"\x00\x00\x01", i)) >= 0:
        idx.append(j); i = j + 3
    ends = idx[1:] + [len(b)]
    return [(b[s + 3] & 0x1F, s + 3, e - s - 3) for s, e in zip(idx, ends)]


def parse_sps(nal: bytes):
    """nal = octets du NAL SPS (en-tete inclus). Retourne (largeur, hauteur, frame_mbs_only)."""
    raw = bytearray()
    for k, x in enumerate(nal):
        if k >= 2 and x == 3 and nal[k - 1] == 0 and nal[k - 2] == 0:
            continue
        raw.append(x)
    b = _Bits(bytes(raw[1:]))
    profile = b.u(8); b.u(8); b.u(8)
    b.ue()
    chroma = 1
    if profile in (100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135):
        chroma = b.ue()
        if chroma == 3:
            b.u(1)
        b.ue(); b.ue(); b.u(1)
        if b.u(1):
            for i in range(8 if chroma != 3 else 12):
                if b.u(1):
                    last, size = 8, 16 if i < 6 else 64
                    nxt = 8
                    for _ in range(size):
                        if nxt:
                            nxt = (last + b.se()) % 256
                        last = last if nxt == 0 else nxt
    b.ue()
    poc = b.ue()
    if poc == 0:
        b.ue()
    elif poc == 1:
        b.u(1); b.se(); b.se()
        for _ in range(b.ue()):
            b.se()
    b.ue(); b.u(1)
    w_mbs, h_map = b.ue() + 1, b.ue() + 1
    frame_mbs_only = b.u(1)
    if not frame_mbs_only:
        b.u(1)
    b.u(1)
    crop = (0, 0, 0, 0)
    if b.u(1):
        crop = (b.ue(), b.ue(), b.ue(), b.ue())
    cx, cy = (2, 2 * (2 - frame_mbs_only)) if chroma == 1 else (1, 2 - frame_mbs_only)
    return (w_mbs * 16 - cx * (crop[0] + crop[1]),
            (2 - frame_mbs_only) * h_map * 16 - cy * (crop[2] + crop[3]), frame_mbs_only)


def video_format_code(height: int, progressive: bool) -> int:
    return {480: 3 if progressive else 1, 576: 7 if progressive else 2,
            720: 5, 1080: 6 if progressive else 4}[height]


def frame_rate(pts_list):
    s = sorted(set(pts_list))
    delta = statistics.median(b - a for a, b in zip(s, s[1:]))
    fps = 90000 / delta
    best = min(FRAME_RATES, key=lambda r: abs(r - fps))
    if abs(best - fps) > 0.2:
        raise ValueError(f"cadence non reconnue : {fps:.3f} i/s")
    return best


def ac3_attrs(payload: bytes):
    """(code de format, code de frequence) depuis le premier en-tete AC3."""
    i = payload.find(b"\x0b\x77")
    fscod = payload[i + 4] >> 6
    acmod = payload[i + 6] >> 5
    if fscod != 0:
        raise ValueError("seul 48 kHz est gere")
    fmt = 1 if acmod == 1 else 3 if acmod == 2 else 6
    return fmt, 1


# ---------------------------------------------------------------- generation
@dataclass
class ClipScan:
    num_packets: int
    pcr_pid: int
    pmt_pid: int
    video: tuple             # (pid, stream_type)
    audio: list              # [(pid, stream_type)]
    width: int
    height: int
    progressive: bool
    fps: float
    start90: int
    end90: int
    eps: list                # [(pts, spn, taille de l'unite d'acces)]
    video_code: int = 0      # octet format/cadence des attributs video
    audio_codes: list = None # [(pid, stream_type, octet format/frequence)]


def scan_clip(data: bytes) -> ClipScan:
    pcr_pid, pmt_pid, streams = parse_psi(data)
    vid = next(s for s in streams if s[0] in VIDEO_TYPES)
    aud = [s for s in streams if s[0] in AUDIO_TYPES]
    vpes = reassemble(data, vid[1])
    sps = None
    eps = []
    for e in vpes:
        for t, off, ln in nal_units(e.data):
            if t == 7:
                if sps is None:
                    sps = parse_sps(e.data[off:off + ln])
                eps.append((e.pts, e.first_packet, len(e.data)))
                break
    w, h, prog = sps
    fps = frame_rate([e.pts for e in vpes if e.pts is not None])
    firsts = [min(e.pts for e in vpes if e.pts is not None)]
    firsts += [min(e.pts for e in reassemble(data, pid) if e.pts is not None) for _, pid in aud]
    audio_codes = []
    for t, pid in aud:
        fmt, rate_code = ac3_attrs(next(e.data for e in reassemble(data, pid)))
        audio_codes.append((pid, t, (fmt << 4) | rate_code))
    video_code = (video_format_code(h, bool(prog)) << 4) | FRAME_RATES[fps][1]
    return ClipScan(len(data) // 192, pcr_pid, pmt_pid, (vid[1], vid[0]), [(p, t) for t, p in aud],
                    w, h, bool(prog), fps, min(firsts),
                    max(e.pts for e in vpes if e.pts is not None), eps, video_code, audio_codes)


def i_end_offset(au_bytes: int) -> int:
    return max(0, min(7, math.ceil(math.log2(au_bytes / 65536)))) if au_bytes > 0 else 0


def build_ep_map(pid: int, eps) -> Cpi:
    st = EpStream(pid=pid, ep_stream_type=1)
    last_c = last_s = None
    for i, (pts, spn, size) in enumerate(eps):
        c, s = (pts >> 19) & 0x3FFF, spn >> 17
        if (c, s) != (last_c, last_s):
            st.coarse.append(EpCoarse(i, c, spn))
            last_c, last_s = c, s
        st.fine.append(EpFine(0, i_end_offset(size), (pts >> 9) & 0x7FF, spn & 0x1FFFF))
    return Cpi(cpi_type=1, streams=[st])


def make_clip_clpi(data: bytes, lang=b"und", pad=b"\x00", aspect_code=None, version=b"0100") -> bytes:
    """Genere le .clpi d'un clip de film. pad : octet de remplissage des attributs de flux
    (multiAVCHD utilise b'0', les disques usuels des zeros)."""
    sc = scan_clip(data)
    start45, end45 = sc.start90 // 2, sc.end90 // 2
    rate = int(len(data) // ((end45 - start45) / 45000))
    aspect = aspect_code or (3 if abs(sc.width / sc.height - 16 / 9) < 0.05 else 2)
    fill = lambda head: head + pad * 12 + bytes(20 - len(head) - 12)
    vattr = fill(bytes([sc.video_code, aspect << 4, 0, 0]))
    streams = [Stream(sc.video[0], sc.video[1], vattr)]
    for pid, typ, code in sc.audio_codes:
        streams.append(Stream(pid, typ, fill(bytes([code]) + lang)))
    c = Clpi(version=version,
             clip_info=ClipInfo(1, 1, 0, rate, sc.num_packets),
             sequence=SequenceInfo([AtcSeq(0, 0, [StcSeq(sc.pcr_pid, 0, start45, end45)])]),
             program=ProgramInfo([Program(0, sc.pmt_pid, 0, streams)]),
             cpi=build_ep_map(sc.video[0], sc.eps))
    return c.pack()
