"""Parseur / ecriveur de index.bdmv (structure INDX)."""
import struct
from dataclasses import dataclass, field
from .reader import Reader, bits

OBJ_HDMV, OBJ_BDJ = 1, 2


@dataclass
class IndexObject:
    object_type: int = OBJ_HDMV      # 1 = HDMV, 2 = BD-J
    access_type: int = 0             # titres uniquement (2 bits)
    playback_type: int = 0           # 2 bits : 1 = film, 2 = interactif
    id_ref: int = 0                  # numero d'objet dans MovieObject.bdmv
    bdj_tail: bytes = b""            # BD-J : 8 octets bruts (nom + reserve)

    @classmethod
    def parse(cls, r: Reader):
        head = r.u32()
        obj = cls(object_type=bits(head, 2, 30), access_type=bits(head, 2, 28))
        if obj.object_type == OBJ_HDMV:
            obj.playback_type = bits(r.u16(), 2, 14)
            obj.id_ref = r.u16()
            r.raw(4)
        else:
            obj.bdj_tail = r.raw(8)
        return obj

    def pack(self) -> bytes:
        head = (self.object_type << 30) | (self.access_type << 28)
        out = struct.pack(">I", head)
        if self.object_type == OBJ_HDMV:
            return out + struct.pack(">HH", self.playback_type << 14, self.id_ref) + bytes(4)
        return out + self.bdj_tail


@dataclass
class IndexBdmv:
    version: bytes = b"0200"
    app_flags: int = 0               # octet de flags de l'AppInfo
    video_format: int = 0            # 4 bits
    frame_rate: int = 0              # 4 bits
    user_data: bytes = bytes(32)
    first_playback: IndexObject = field(default_factory=IndexObject)
    top_menu: IndexObject = field(default_factory=IndexObject)
    titles: list = field(default_factory=list)

    @classmethod
    def parse(cls, data: bytes):
        r = Reader(data)
        if r.raw(4) != b"INDX":
            raise ValueError("signature INDX absente")
        idx = cls(version=r.raw(4))
        index_start = r.u32()
        r.u32()                       # adresse des extensions (ignorees ici)
        r.seek(40)
        r.u32()                       # longueur AppInfo
        idx.app_flags = r.u8()
        fmt = r.u8()
        idx.video_format, idx.frame_rate = bits(fmt, 4, 4), bits(fmt, 4, 0)
        idx.user_data = r.raw(32)
        r.seek(index_start)
        r.u32()                       # longueur de la table
        idx.first_playback = IndexObject.parse(r)
        idx.top_menu = IndexObject.parse(r)
        n = r.u16()
        idx.titles = [IndexObject.parse(r) for _ in range(n)]
        return idx

    def pack(self) -> bytes:
        app = struct.pack(">BB", self.app_flags,
                          (self.video_format << 4) | self.frame_rate) + self.user_data
        table = (self.first_playback.pack() + self.top_menu.pack()
                 + struct.pack(">H", len(self.titles))
                 + b"".join(t.pack() for t in self.titles))
        index_start = 40 + 4 + len(app)
        return (b"INDX" + self.version + struct.pack(">II", index_start, 0)
                + bytes(24) + struct.pack(">I", len(app)) + app
                + struct.pack(">I", len(table)) + table)

    def dump(self) -> str:
        def d(o):
            if o.object_type == OBJ_HDMV:
                return f"HDMV playback_type={o.playback_type} object={o.id_ref}"
            return f"BD-J {o.bdj_tail[:5]!r}"
        lines = [f"version        : {self.version.decode()}",
                 f"video_format   : {self.video_format}  frame_rate: {self.frame_rate}",
                 f"user_data      : {self.user_data.rstrip(bytes(1)).decode('ascii', 'replace')}",
                 f"First Playback : {d(self.first_playback)}",
                 f"Top Menu       : {d(self.top_menu)}",
                 f"titres         : {len(self.titles)}"]
        lines += [f"  titre {i + 1}: {d(t)} access_type={t.access_type}"
                  for i, t in enumerate(self.titles)]
        return "\n".join(lines)
