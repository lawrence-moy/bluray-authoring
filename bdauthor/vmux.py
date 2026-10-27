"""Multiplexeur M2TS (paquets sources de 192 octets) pour un film H.264 + AC3.

Regles reprises du film de reference (00000.m2ts, multiAVCHD) :
 - PAT, PMT, SIT en tete puis repetees (toutes les ~0,25 s, aux frontieres de PES), compteur de continuite par PID ;
 - PCR dans des paquets dedies (PID 0x1001, adaptation seule), au moins toutes les 90 ms ; valeur = ATS du paquet ;
 - un PES par unite d'acces video (stream_id 0xE0, longueur 0 si > 65 535 octets), un PES par trame AC3 ;
   PTS seul quand DTS = PTS, sinon PTS + DTS ; le dernier paquet de chaque PES est complete par un champ d'adaptation ;
 - les paquets d'un PES arrivent ~0,48 s avant son heure de decodage (DTS video, PTS audio), debit d'arrivee = peak_rate x 50 o/s,
   les PES etant emis dans l'ordre des heures de decodage (l'audio s'intercale entre deux PES video) ;
 - bourrage final : paquets nuls jusqu'a un multiple de 32 paquets sources.
Differences volontaires : audio en stream_id 0xBD (private_stream_1, usage courant) au lieu de 0xFD du film de reference ;
la « avance » est augmentee automatiquement si une image arriverait trop tard.
"""
import struct
from dataclasses import dataclass
from .mux import make_pat, make_pmt, make_sit, _psi_packet, _pcr_packet, _ts5

PAYLOAD = 184
STREAM_ID_VIDEO, STREAM_ID_AUDIO = 0xE0, 0xBD
AC3_FRAME_TICKS = 2880                  # 1536 echantillons a 48 kHz, en ticks de 90 kHz
AC3_WORDS_48K = [64, 64, 80, 80, 96, 96, 112, 112, 128, 128, 160, 160, 192, 192, 224, 224, 256, 256, 320, 320,
                 384, 384, 448, 448, 512, 512, 640, 640, 768, 768, 896, 896, 1024, 1024, 1152, 1152, 1280, 1280]


def build_pes(stream_id, pts, dts, data):
    if dts is None or dts == pts:
        flags, ts = 0x80, _ts5(0b0010, pts)
    else:
        flags, ts = 0xC0, _ts5(0b0011, pts) + _ts5(0b0001, dts)
    length = 3 + len(ts) + len(data)
    if length > 0xFFFF:
        if stream_id != STREAM_ID_VIDEO:
            raise ValueError("PES trop long")
        length = 0
    return b"\x00\x00\x01" + bytes([stream_id]) + struct.pack(">HBBB", length, 0x84, flags, len(ts)) + ts + data


def packetize(pes, pid, cc):
    out, pos, first = [], 0, True
    while pos < len(pes):
        chunk = pes[pos:pos + PAYLOAD]
        pos += len(chunk)
        b1 = (0x40 if first else 0) | (pid >> 8)
        first = False
        if len(chunk) == PAYLOAD:
            out.append(bytes([0x47, b1, pid & 0xFF, 0x10 | cc]) + chunk)
        else:
            alen = PAYLOAD - len(chunk) - 1
            af = bytes([alen]) + (b"\x00" + b"\xff" * (alen - 1) if alen else b"")
            out.append(bytes([0x47, b1, pid & 0xFF, 0x30 | cc]) + af + chunk)
        cc = (cc + 1) & 15
    return out, cc


@dataclass
class _Item:
    kind: str
    deadline: int          # heure de decodage, ticks de 27 MHz
    packets: list
    size: int              # octets de charge utile elementaire


def _schedule(items, dt, lead, pcr_i, psi_i, psi_sections, pmt_pid, margin):
    out, state = [], {"t": items[0].deadline - lead - 4 * dt, "psi": 0.0, "pcr": 0.0}
    cc = {0: 0, pmt_pid: 0, 0x1F: 0}

    def psi():
        state["psi"] = state["t"]
        for pid, sec in zip((0, pmt_pid, 0x1F), psi_sections):
            out.append((state["t"], _psi_packet(pid, sec, cc[pid])))
            cc[pid] = (cc[pid] + 1) & 15
            state["t"] += dt

    def pcr():
        state["pcr"] = state["t"]
        out.append((state["t"], None))
        state["t"] += dt

    psi(); pcr()
    late, finish = float("-inf"), []
    for it in items:
        want = it.deadline - lead
        while state["t"] < want:                       # attente : les PCR et PSI restent dans les delais
            due_pcr, due_psi = state["pcr"] + pcr_i, state["psi"] + psi_i
            due = min(due_pcr, due_psi)
            if due >= want:
                state["t"] = want
                break
            state["t"] = max(state["t"], due)
            pcr() if due_pcr <= due_psi else psi()
        if state["t"] - state["psi"] >= psi_i:
            psi()
        for pkt in it.packets:
            if state["t"] - state["pcr"] >= pcr_i:
                pcr()
            out.append((state["t"], pkt))
            state["t"] += dt
        finish.append(state["t"])
        late = max(late, state["t"] - it.deadline + margin)
    return out, late, finish


def mux_film(video, audio, pmt_streams, video_pid=0x1011, audio_pid=0x1100, pmt_pid=0x0100, pcr_pid=0x1001,
             peak_rate=88750, lead_s=0.483, pcr_interval_s=0.09, psi_interval_s=0.25, margin_s=0.002):
    """video : [(pts, dts, donnees Annex B)] en ordre de decodage ; audio : [(pts, trame AC3)] ; temps en 90 kHz.
    pmt_streams : [(stream_type, pid, descripteurs)] pour la PMT. Retourne (octets .m2ts, statistiques)."""
    rate = peak_rate * 50
    dt = 188 * 27_000_000 / rate
    items, ccs = [], {video_pid: 0, audio_pid: 0}
    for kind, pid, sid, pts, dts, data in ([("v", video_pid, STREAM_ID_VIDEO, p, d, x) for p, d, x in video]
                                           + [("a", audio_pid, STREAM_ID_AUDIO, p, None, x) for p, x in audio]):
        items.append((((dts if dts is not None else pts) * 300), kind, pid, build_pes(sid, pts, dts, data), len(data)))
    items.sort(key=lambda x: (x[0], x[1]))
    for i, (dl, kind, pid, pes, size) in enumerate(items):
        pk, ccs[pid] = packetize(pes, pid, ccs[pid])
        items[i] = _Item(kind, dl, pk, size)
    sections = (make_pat(pmt_pid), make_pmt(pmt_streams, pcr_pid), make_sit(peak_rate))
    lead, margin = lead_s * 27_000_000, margin_s * 27_000_000
    for _ in range(40):
        out, late, finish = _schedule(items, dt, lead, pcr_interval_s * 27e6, psi_interval_s * 27e6, sections, pmt_pid, margin)
        if late <= 0:
            break
        lead += late + 27_000
    else:
        raise ValueError("debit d'arrivee insuffisant pour tenir les heures de decodage")
    # tampon video : octets arrives et pas encore decodes, estimes aux heures de decodage
    ev = [(f, +it.size) for it, f in zip(items, finish) if it.kind == "v"] + [(it.deadline, -it.size) for it in items if it.kind == "v"]
    occ = peak = 0
    for _, delta in sorted(ev, key=lambda e: (e[0], -e[1])):
        occ += delta
        peak = max(peak, occ)
    t_end = out[-1][0] + dt
    while len(out) % 32:
        out.append((t_end, bytes([0x47, 0x1F, 0xFF, 0x10]) + bytes(PAYLOAD)))
        t_end += dt
    buf, pcr_times, last_ats = bytearray(), [], -1
    for t, pkt in out:
        t = int(t)
        if pkt is None:
            pkt = _pcr_packet(t)
            pcr_times.append(t)
        ats = t & 0x3FFFFFFF
        buf += struct.pack(">I", ats) + pkt
    stats = {"paquets": len(out), "octets": len(buf), "avance_s": lead / 27e6, "debit_pic_Mbps": rate * 8 / 1e6,
             "ecart_pcr_max_ms": max((b - a) / 27000 for a, b in zip(pcr_times, pcr_times[1:])) if len(pcr_times) > 1 else 0,
             "tampon_video_max_octets": peak, "pcr": len(pcr_times)}
    return bytes(buf), stats
