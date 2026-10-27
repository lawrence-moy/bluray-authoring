"""Parseur / ecriveur de fichiers .mpls (playlists, structure MPLS).

Decode : en-tete, PlayItems (clip, temps), SubPaths/SubPlayItems, marques (chapitres).
Conserve tel quel (octets bruts) : AppInfo et la queue de chaque PlayItem
(masque UO, flags, table STN des flux), pas encore decodees.
"""
import struct
from dataclasses import dataclass, field
from .reader import Reader, bits

TICKS = 45000  # horloge des temps de playlist : 45 kHz

SUBPATH_TYPES = {2: "audio", 3: "menu IG (presentation)"}   # autres types : non verifies
MARK_TYPES = {1: "chapitre (entry mark)", 2: "link point"}


@dataclass
class PlayItem:
    clip: str = "00000"
    codec: str = "M2TS"
    is_multi_angle: int = 0
    connection_condition: int = 1
    stc_id: int = 0
    in_time: int = 0
    out_time: int = 0
    tail: bytes = b""          # UO mask, flags, still, table STN (brut)

    @classmethod
    def parse(cls, r: Reader):
        length = r.u16()
        end = r.pos + length
        it = cls(clip=r.raw(5).decode("ascii"), codec=r.raw(4).decode("ascii"))
        flags = r.u16()
        it.is_multi_angle, it.connection_condition = bits(flags, 1, 4), bits(flags, 4, 0)
        it.stc_id = r.u8()
        it.in_time, it.out_time = r.u32(), r.u32()
        it.tail = r.raw(end - r.pos)
        return it

    def pack(self) -> bytes:
        body = (self.clip.encode() + self.codec.encode()
                + struct.pack(">HB", (self.is_multi_angle << 4) | self.connection_condition, self.stc_id)
                + struct.pack(">II", self.in_time, self.out_time) + self.tail)
        return struct.pack(">H", len(body)) + body


@dataclass
class SubPlayItem:
    clip: str = "00000"
    codec: str = "M2TS"
    connection_condition: int = 1
    is_multi_clip: int = 0
    stc_id: int = 0
    in_time: int = 0
    out_time: int = 0
    sync_playitem_id: int = 0
    sync_start_pts: int = 0

    @classmethod
    def parse(cls, r: Reader):
        r.u16()
        s = cls(clip=r.raw(5).decode("ascii"), codec=r.raw(4).decode("ascii"))
        flags = r.u32()
        s.connection_condition, s.is_multi_clip = bits(flags, 4, 1), bits(flags, 1, 0)
        s.stc_id = r.u8()
        s.in_time, s.out_time = r.u32(), r.u32()
        s.sync_playitem_id, s.sync_start_pts = r.u16(), r.u32()
        return s

    def pack(self) -> bytes:
        body = (self.clip.encode() + self.codec.encode()
                + struct.pack(">IB", (self.connection_condition << 1) | self.is_multi_clip, self.stc_id)
                + struct.pack(">IIHI", self.in_time, self.out_time,
                              self.sync_playitem_id, self.sync_start_pts))
        return struct.pack(">H", len(body)) + body


@dataclass
class SubPath:
    subpath_type: int = 3
    is_repeat: int = 0
    items: list = field(default_factory=list)

    @classmethod
    def parse(cls, r: Reader):
        r.u32()                         # longueur
        r.u8()
        sp = cls(subpath_type=r.u8())
        sp.is_repeat = bits(r.u16(), 1, 0)
        r.u8()
        n = r.u8()
        sp.items = [SubPlayItem.parse(r) for _ in range(n)]
        return sp

    def pack(self) -> bytes:
        body = (struct.pack(">BBHBB", 0, self.subpath_type, self.is_repeat, 0, len(self.items))
                + b"".join(i.pack() for i in self.items))
        return struct.pack(">I", len(body)) + body


@dataclass
class Mark:
    mark_type: int = 1
    playitem_ref: int = 0
    time: int = 0
    es_pid: int = 0xFFFF
    duration: int = 0

    @classmethod
    def parse(cls, r: Reader):
        r.u8()
        return cls(r.u8(), r.u16(), r.u32(), r.u16(), r.u32())

    def pack(self) -> bytes:
        return struct.pack(">BBHIHI", 0, self.mark_type, self.playitem_ref,
                           self.time, self.es_pid, self.duration)


@dataclass
class Mpls:
    version: bytes = b"0200"
    app_info: bytes = bytes(14)
    playitems: list = field(default_factory=list)
    subpaths: list = field(default_factory=list)
    marks: list = field(default_factory=list)
    extension: bytes = b""              # donnees d'extension brutes (si presentes)

    @classmethod
    def parse(cls, data: bytes):
        r = Reader(data)
        if r.raw(4) != b"MPLS":
            raise ValueError("signature MPLS absente")
        m = cls(version=r.raw(4))
        pl_start, mark_start, ext_start = r.u32(), r.u32(), r.u32()
        r.seek(40)
        m.app_info = r.raw(r.u32())
        r.seek(pl_start)
        r.u32(); r.u16()
        n_items, n_sub = r.u16(), r.u16()
        m.playitems = [PlayItem.parse(r) for _ in range(n_items)]
        m.subpaths = [SubPath.parse(r) for _ in range(n_sub)]
        r.seek(mark_start)
        r.u32()
        m.marks = [Mark.parse(r) for _ in range(r.u16())]
        if ext_start:
            m.extension = data[ext_start:]
        return m

    def pack(self) -> bytes:
        app = struct.pack(">I", len(self.app_info)) + self.app_info
        pl_body = (struct.pack(">HHH", 0, len(self.playitems), len(self.subpaths))
                   + b"".join(i.pack() for i in self.playitems)
                   + b"".join(s.pack() for s in self.subpaths))
        pl = struct.pack(">I", len(pl_body)) + pl_body
        mk_body = struct.pack(">H", len(self.marks)) + b"".join(k.pack() for k in self.marks)
        mk = struct.pack(">I", len(mk_body)) + mk_body
        pl_start = 40 + len(app)
        mark_start = pl_start + len(pl)
        ext_start = mark_start + len(mk) if self.extension else 0
        head = b"MPLS" + self.version + struct.pack(">III", pl_start, mark_start, ext_start) + bytes(20)
        return head + app + pl + mk + self.extension

    def dump(self) -> str:
        s = lambda t: f"{t / TICKS:.3f}s"
        out = [f"version {self.version.decode()}"]
        for i, p in enumerate(self.playitems):
            out.append(f"PlayItem {i}: clip {p.clip}.{p.codec}  in={s(p.in_time)} out={s(p.out_time)} "
                       f"duree={s(p.out_time - p.in_time)} connexion={p.connection_condition} "
                       f"({len(p.tail)} octets bruts : UO/STN)")
        for i, sp in enumerate(self.subpaths):
            out.append(f"SubPath {i}: type {sp.subpath_type} "
                       f"= {SUBPATH_TYPES.get(sp.subpath_type, '?')}, repeat={sp.is_repeat}")
            for it in sp.items:
                out.append(f"  SubPlayItem: clip {it.clip}.{it.codec} in={s(it.in_time)} "
                           f"out={s(it.out_time)} duree={s(it.out_time - it.in_time)}")
        for i, k in enumerate(self.marks):
            out.append(f"Mark {i}: {MARK_TYPES.get(k.mark_type, k.mark_type)} "
                       f"playitem={k.playitem_ref} t={s(k.time)}")
        return "\n".join(out)
