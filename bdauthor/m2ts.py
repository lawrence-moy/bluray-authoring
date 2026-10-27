"""Lecture de fichiers .m2ts : paquets source de 192 octets, PES, segments IG.

Un paquet source = 4 octets (2 bits copy_permission + 30 bits ATS) + 188 octets TS.
"""
from dataclasses import dataclass, field

SRC = 192


@dataclass
class TsPacket:
    index: int
    copy_permission: int
    ats: int                       # horodatage d'arrivee (27 MHz, 30 bits)
    pid: int
    pusi: int                      # payload_unit_start_indicator
    afc: int                       # adaptation_field_control (1 payload, 2 adaptation, 3 les deux)
    cc: int                        # continuity counter
    pcr: object                    # PCR en ticks 27 MHz, ou None
    payload: bytes


def iter_packets(data: bytes):
    if len(data) % SRC:
        raise ValueError("taille non multiple de 192")
    for n in range(len(data) // SRC):
        p = data[n * SRC:(n + 1) * SRC]
        head, ts = int.from_bytes(p[:4], "big"), p[4:]
        if ts[0] != 0x47:
            raise ValueError(f"octet de synchro absent au paquet {n}")
        afc = (ts[3] >> 4) & 3
        pos, pcr = 4, None
        if afc in (2, 3):
            alen = ts[4]
            if alen >= 7 and ts[5] & 0x10:
                v = int.from_bytes(ts[6:12], "big")
                pcr = (v >> 15) * 300 + (v & 0x1FF)
            pos = 5 + alen
        payload = ts[pos:] if afc in (1, 3) else b""
        yield TsPacket(n, head >> 30, head & 0x3FFFFFFF, ((ts[1] & 0x1F) << 8) | ts[2],
                       (ts[1] >> 6) & 1, afc, ts[3] & 15, pcr, payload)


@dataclass
class Pes:
    pid: int
    first_packet: int
    stream_id: int
    pts: object
    dts: object
    data: bytes                    # charge utile apres l'en-tete PES
    declared_length: int
    header: bytes = b""            # en-tete PES brut (pour reecriture)


def _ts33(b: bytes) -> int:
    return (((b[0] >> 1) & 7) << 30) | (b[1] << 22) | ((b[2] >> 1) << 15) | (b[3] << 7) | (b[4] >> 1)


def parse_pes(pid: int, first: int, raw: bytes) -> Pes:
    if raw[:3] != b"\x00\x00\x01":
        raise ValueError("start code PES absent")
    stream_id, plen = raw[3], int.from_bytes(raw[4:6], "big")
    flags, hlen = raw[7], raw[8]
    pts = dts = None
    if flags & 0x80:
        pts = _ts33(raw[9:14])
    if flags & 0x40:
        dts = _ts33(raw[14:19])
    return Pes(pid, first, stream_id, pts, dts, raw[9 + hlen:], plen, raw[:9 + hlen])


def reassemble(data: bytes, pid: int):
    """Liste des PES complets d'un PID."""
    out, buf, first = [], None, 0
    for p in iter_packets(data):
        if p.pid != pid or not p.payload:
            continue
        if p.pusi:
            if buf is not None:
                out.append(parse_pes(pid, first, bytes(buf)))
            buf, first = bytearray(p.payload), p.index
        elif buf is not None:
            buf += p.payload
    if buf is not None:
        out.append(parse_pes(pid, first, bytes(buf)))
    return out


SEGMENT_NAMES = {0x14: "PDS", 0x15: "ODS", 0x16: "PCS", 0x17: "WDS", 0x18: "ICS", 0x80: "END"}


@dataclass
class Segment:
    seg_type: int
    payload: bytes

    @property
    def name(self):
        return SEGMENT_NAMES.get(self.seg_type, hex(self.seg_type))

    def pack(self) -> bytes:
        return bytes([self.seg_type]) + len(self.payload).to_bytes(2, "big") + self.payload


def split_segments(data: bytes):
    segs, pos = [], 0
    while pos < len(data):
        t, n = data[pos], int.from_bytes(data[pos + 1:pos + 3], "big")
        if pos + 3 + n > len(data):
            raise ValueError(f"segment tronque a l'offset {pos}")
        segs.append(Segment(t, data[pos + 3:pos + 3 + n]))
        pos += 3 + n
    return segs
