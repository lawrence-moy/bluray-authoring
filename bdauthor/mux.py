"""Multiplexeur M2TS pour un clip de menu IG (sous-TS).

Regles retrouvees sur le disque de reference (80002.m2ts) :
 - ordre : PAT, PMT, SIT, PCR, puis les PES du flux IG, puis bourrage (paquets
   nuls) jusqu'a un multiple de 32 paquets sources (6144 octets) ;
 - ATS du paquet n = floor(n * 188 * 27e6 / debit), debit = peak_rate * 50 o/s,
   ou peak_rate est celui declare dans la SIT ;
 - valeur du PCR = ATS du paquet qui le porte ;
 - dernier paquet d'un PES complete par un champ d'adaptation de bourrage (0xFF) ;
 - compteurs de continuite : un compteur par PID, incremente a chaque paquet
   portant une charge utile ; paquets PCR et nuls : compteur a 0 ;
 - PES IG : stream_id 0xBD, PTS+DTS (flags 0xC0) ou PTS seul (0x80) ;
 - duree de decodage d'un ODS : ceil(largeur * hauteur * 90000 / 8e6) ;
 - ICS : DTS = PTS - ceil(largeur_video * hauteur_video * 90000 / 8e6).
"""
import math
import struct
from dataclasses import dataclass

TS_SIZE, SRC_SIZE, PAYLOAD = 188, 192, 184
STREAM_ID_IG = 0xBD
ALIGN_PACKETS = 32
TICKS_90K_DIV = 8_000_000
PMT_DESCRIPTORS = bytes.fromhex("050448444d56" "88040ffffcfc")   # 'HDMV' + copy control


def crc32_mpeg(data: bytes) -> int:
    c = 0xFFFFFFFF
    for x in data:
        c ^= x << 24
        for _ in range(8):
            c = ((c << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if c & 0x80000000 else (c << 1) & 0xFFFFFFFF
    return c


def _section(body: bytes) -> bytes:
    return body + struct.pack(">I", crc32_mpeg(body))


def make_pat(pmt_pid=0x0100, program=1, tsid=1, network_pid=0x001F) -> bytes:
    prog = struct.pack(">HHHH", 0, 0xE000 | network_pid, program, 0xE000 | pmt_pid)
    body = bytes([0x00]) + struct.pack(">H", 0xB000 | (5 + len(prog) + 4)) \
        + struct.pack(">HBBB", tsid, 0xC1, 0, 0) + prog
    return _section(body)


def make_pmt(streams, pcr_pid=0x1001, program=1) -> bytes:
    """streams : liste de (stream_type, pid) ou (stream_type, pid, descripteurs_ES_en_octets)."""
    loop = b""
    for st in streams:
        t, pid, desc = (st + (b"",))[:3] if len(st) == 2 else st
        loop += struct.pack(">BHH", t, 0xE000 | pid, 0xF000 | len(desc)) + desc
    info = PMT_DESCRIPTORS
    n = 9 + len(info) + len(loop) + 4
    body = bytes([0x02]) + struct.pack(">H", 0xB000 | n) \
        + struct.pack(">HBBBHH", program, 0xC1, 0, 0, 0xE000 | pcr_pid, 0xF000 | len(info)) + info + loop
    return _section(body)


def make_sit(peak_rate: int, program=1) -> bytes:
    """peak_rate en unites de 50 octets/s."""
    desc = bytes([0x63, 0x08]) + (0xC00000 | peak_rate).to_bytes(3, "big") + bytes.fromhex("ffffffffff")
    body = bytes([0x7F]) + struct.pack(">H", 0xF000 | (2 + 3 + 2 + len(desc) + 4 + 4)) \
        + struct.pack(">HBBBH", 0xFFFF, 0xC1, 0, 0, 0xF000 | len(desc)) + desc \
        + struct.pack(">HH", program, 0x8000)
    return _section(body)


@dataclass
class PesSpec:
    pts: int
    dts: object            # int ou None
    data: bytes            # segments IG concatenes


def _ts5(prefix: int, v: int) -> bytes:
    return bytes([(prefix << 4) | (((v >> 30) & 7) << 1) | 1, (v >> 22) & 0xFF,
                  (((v >> 15) & 0x7F) << 1) | 1, (v >> 7) & 0xFF, ((v & 0x7F) << 1) | 1])


def build_pes(spec: PesSpec, stream_id=STREAM_ID_IG) -> bytes:
    if spec.dts is None:
        flags, ts = 0x80, _ts5(0b0010, spec.pts)
    else:
        flags, ts = 0xC0, _ts5(0b0011, spec.pts) + _ts5(0b0001, spec.dts)
    length = 3 + len(ts) + len(spec.data)
    if length > 0xFFFF:
        raise ValueError("PES trop long pour un champ de longueur 16 bits")
    return b"\x00\x00\x01" + bytes([stream_id]) + struct.pack(">HBBB", length, 0x85, flags, len(ts)) \
        + ts + spec.data


def _plane_ticks(w: int, h: int) -> int:
    return math.ceil(w * h * 90000 / TICKS_90K_DIV)


def ig_pes_specs(ics: bytes, pds: bytes, ods: list, ics_pts: int, video_size=(1920, 1080)):
    """ics, pds : segments complets (type + longueur + charge). ods : liste de (segment, largeur, hauteur).
    Calcule PTS/DTS selon les regles observees."""
    t = ics_pts - _plane_ticks(*video_size)
    specs = [PesSpec(ics_pts, t, ics), PesSpec(t, None, pds)]
    for seg, w, h in ods:
        specs.append(PesSpec(t + _plane_ticks(w, h), t, seg))
        t += _plane_ticks(w, h)
    specs.append(PesSpec(t, None, bytes([0x80, 0, 0])))
    return specs


def ats_of(n: int, peak_rate: int) -> int:
    return (n * TS_SIZE * 27_000_000 // (peak_rate * 50)) & 0x3FFFFFFF


def _pcr_packet(pcr: int) -> bytes:
    v = ((pcr // 300) << 15) | (0x3F << 9) | (pcr % 300)
    return struct.pack(">I", 0x47100120) + bytes([183, 0x10]) + v.to_bytes(6, "big") + b"\xff" * 176


def _psi_packet(pid: int, section: bytes, cc: int = 0) -> bytes:
    pl = (b"\x00" + section).ljust(PAYLOAD, b"\xff")
    return bytes([0x47, 0x60 | (pid >> 8), pid & 0xFF, 0x10 | (cc & 15)]) + pl


def mux_ig_clip(pes_specs, ig_pid=0x1400, pmt_pid=0x0100, pcr_pid=0x1001, peak_rate=107679,
                ig_stream_type=0x91, pcr_interval=2_430_000, pcr_extra_at=None) -> bytes:
    """Retourne le contenu complet d'un .m2ts de menu.

    pcr_extra_at : indices de paquets ou inserer un PCR (rejeu exact d'un disque de reference) ;
    sinon un PCR est insere des que `pcr_interval` ticks (27 MHz) se sont ecoules (90 ms par defaut).
    """
    pkts = [_psi_packet(0, make_pat(pmt_pid)), _psi_packet(pmt_pid, make_pmt([(ig_stream_type, ig_pid)], pcr_pid)),
            _psi_packet(0x1F, make_sit(peak_rate)), None]          # None = PCR initial
    cc, last_pcr_ats = 0, 0
    extra = set(pcr_extra_at or [])

    def pcr_slot():
        pkts.append(None)

    for spec in pes_specs:
        pes, pos, first = build_pes(spec), 0, True
        while pos < len(pes):
            idx = len(pkts)
            if idx in extra or (pcr_extra_at is None and ats_of(idx, peak_rate) - last_pcr_ats >= pcr_interval):
                last_pcr_ats = ats_of(idx, peak_rate)
                pcr_slot()
                continue
            chunk = pes[pos:pos + PAYLOAD]
            pos += len(chunk)
            head = 0x4000 if first else 0
            first = False
            if len(chunk) == PAYLOAD:
                pkts.append(bytes([0x47, (head >> 8) | (ig_pid >> 8), ig_pid & 0xFF, 0x10 | cc]) + chunk)
            else:
                alen = PAYLOAD - len(chunk) - 1
                af = bytes([alen]) + (b"\x00" + b"\xff" * (alen - 1) if alen else b"")
                pkts.append(bytes([0x47, (head >> 8) | (ig_pid >> 8), ig_pid & 0xFF, 0x30 | cc]) + af + chunk)
            cc = (cc + 1) & 15
    while len(pkts) % ALIGN_PACKETS:
        pkts.append(bytes([0x47, 0x1F, 0xFF, 0x10]) + bytes(PAYLOAD))
    out = bytearray()
    for n, p in enumerate(pkts):
        ats = ats_of(n, peak_rate)
        if p is None:
            p = _pcr_packet(ats)
        out += struct.pack(">I", ats) + p
    return bytes(out)
