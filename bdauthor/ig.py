"""Segments du flux IG (Interactive Graphics) : ICS, PDS, ODS.

Chaque classe a parse(payload) / pack() ; les octets reserves sont conserves
pour que l'aller-retour soit exact.
"""
import struct
from dataclasses import dataclass, field
from .reader import Reader, bits
from .mobj import Command


# ------------------------------------------------------------------ PDS
@dataclass
class Pds:
    palette_id: int = 0
    version: int = 0
    entries: dict = field(default_factory=dict)      # index -> (Y, Cr, Cb, alpha)

    @classmethod
    def parse(cls, b: bytes):
        p = cls(b[0], b[1])
        for i in range(2, len(b), 5):
            p.entries[b[i]] = tuple(b[i + 1:i + 5])
        return p

    def pack(self) -> bytes:
        return bytes([self.palette_id, self.version]) + b"".join(
            bytes((i,) + v) for i, v in self.entries.items())

    def rgba(self, index: int):
        """Conversion YCrCb (BT.709, plage limitee) -> RGBA."""
        y, cr, cb, a = self.entries.get(index, (16, 128, 128, 0))
        c = lambda v: max(0, min(255, round(v)))
        yy = 1.164 * (y - 16)
        return (c(yy + 1.793 * (cr - 128)), c(yy - 0.213 * (cb - 128) - 0.533 * (cr - 128)),
                c(yy + 2.112 * (cb - 128)), a)


# ------------------------------------------------------------------ ODS + RLE
def rle_decode(data: bytes, width: int, height: int):
    rows, row, pos = [], [], 0
    while pos < len(data):
        b = data[pos]; pos += 1
        if b:
            row.append(b); continue
        b = data[pos]; pos += 1
        if b == 0:
            rows.append(row); row = []
            continue
        kind, n = b & 0xC0, b & 0x3F
        if kind in (0x40, 0xC0):
            n = (n << 8) | data[pos]; pos += 1
        color = 0
        if kind in (0x80, 0xC0):
            color = data[pos]; pos += 1
        row.extend([color] * n)
    if row:
        rows.append(row)
    if len(rows) != height or any(len(r) != width for r in rows):
        raise ValueError("RLE incoherent avec la taille declaree")
    return rows


def rle_encode(rows) -> bytes:
    out = bytearray()
    for row in rows:
        i = 0
        while i < len(row):
            c, n = row[i], 1
            while i + n < len(row) and row[i + n] == c and n < 0x3FFF:
                n += 1
            i += n
            if c and n <= 2:
                out += bytes([c]) * n
            elif c == 0 and n < 64:
                out += bytes([0, n])
            elif c == 0:
                out += bytes([0, 0x40 | (n >> 8), n & 0xFF])
            elif n < 64:
                out += bytes([0, 0x80 | n, c])
            else:
                out += bytes([0, 0xC0 | (n >> 8), n & 0xFF, c])
        out += b"\x00\x00"
    return bytes(out)


@dataclass
class Ods:
    object_id: int = 0
    version: int = 0
    sequence_flag: int = 0xC0       # 0x80 premier, 0x40 dernier
    width: int = 0
    height: int = 0
    rle: bytes = b""

    @classmethod
    def parse(cls, b: bytes):
        o = cls(int.from_bytes(b[0:2], "big"), b[2], b[3])
        if o.sequence_flag & 0x80:
            dlen = int.from_bytes(b[4:7], "big")
            o.width, o.height = struct.unpack(">HH", b[7:11])
            o.rle = b[11:]
            if dlen != len(o.rle) + 4:
                raise ValueError("data_length incoherent")
        else:
            raise ValueError("ODS fragmente : non gere")
        return o

    def pack(self) -> bytes:
        return (struct.pack(">HBB", self.object_id, self.version, self.sequence_flag)
                + (len(self.rle) + 4).to_bytes(3, "big")
                + struct.pack(">HH", self.width, self.height) + self.rle)

    def pixels(self):
        return rle_decode(self.rle, self.width, self.height)


# ------------------------------------------------------------------ ICS
@dataclass
class Window:
    window_id: int; x: int; y: int; w: int; h: int


@dataclass
class CompObject:
    object_id_ref: int; window_id_ref: int; flags: int; x: int; y: int
    crop: tuple = None


@dataclass
class Effect:
    duration: int; palette_id_ref: int; objects: list


@dataclass
class EffectSequence:
    windows: list = field(default_factory=list)
    effects: list = field(default_factory=list)

    @classmethod
    def parse(cls, r: Reader):
        s = cls()
        for _ in range(r.u8()):
            s.windows.append(Window(r.u8(), r.u16(), r.u16(), r.u16(), r.u16()))
        for _ in range(r.u8()):
            dur = int.from_bytes(r.raw(3), "big")
            pal, n = r.u8(), r.u8()
            objs = []
            for _ in range(n):
                o = CompObject(r.u16(), r.u8(), r.u8(), r.u16(), r.u16())
                if o.flags & 0x80:
                    o.crop = (r.u16(), r.u16(), r.u16(), r.u16())
                objs.append(o)
            s.effects.append(Effect(dur, pal, objs))
        return s

    def pack(self) -> bytes:
        out = bytes([len(self.windows)])
        out += b"".join(struct.pack(">BHHHH", w.window_id, w.x, w.y, w.w, w.h) for w in self.windows)
        out += bytes([len(self.effects)])
        for e in self.effects:
            out += e.duration.to_bytes(3, "big") + bytes([e.palette_id_ref, len(e.objects)])
            for o in e.objects:
                out += struct.pack(">HBBHH", o.object_id_ref, o.window_id_ref, o.flags, o.x, o.y)
                if o.crop:
                    out += struct.pack(">HHHH", *o.crop)
        return out


@dataclass
class Button:
    button_id: int = 0
    numeric_select_value: int = 0
    auto_action_flags: int = 0
    x: int = 0
    y: int = 0
    upper: int = 0; lower: int = 0; left: int = 0; right: int = 0
    normal: tuple = (0, 0, 0)               # objet debut, objet fin, flags
    selected: tuple = (0, 0, 0, 0)          # son, debut, fin, flags
    activated: tuple = (0, 0, 0)            # son, debut, fin
    commands: list = field(default_factory=list)

    @classmethod
    def parse(cls, r: Reader):
        b = cls(r.u16(), r.u16(), r.u8(), r.u16(), r.u16(), r.u16(), r.u16(), r.u16(), r.u16())
        b.normal = (r.u16(), r.u16(), r.u8())
        b.selected = (r.u8(), r.u16(), r.u16(), r.u8())
        b.activated = (r.u8(), r.u16(), r.u16())
        b.commands = [Command.parse(r) for _ in range(r.u16())]
        return b

    def pack(self) -> bytes:
        out = struct.pack(">HHBHHHHHH", self.button_id, self.numeric_select_value,
                          self.auto_action_flags, self.x, self.y,
                          self.upper, self.lower, self.left, self.right)
        out += struct.pack(">HHB", *self.normal) + struct.pack(">BHHB", *self.selected)
        out += struct.pack(">BHH", *self.activated) + struct.pack(">H", len(self.commands))
        return out + b"".join(c.pack() for c in self.commands)


@dataclass
class Bog:
    default_valid_button: int = 0
    buttons: list = field(default_factory=list)


@dataclass
class Page:
    page_id: int = 0
    version: int = 0
    uo_mask: bytes = bytes(8)
    in_effects: EffectSequence = field(default_factory=EffectSequence)
    out_effects: EffectSequence = field(default_factory=EffectSequence)
    frame_rate_code: int = 0
    default_selected: int = 0
    default_activated: int = 0
    palette_id_ref: int = 0
    bogs: list = field(default_factory=list)

    @classmethod
    def parse(cls, r: Reader):
        p = cls(r.u8(), r.u8(), r.raw(8))
        p.in_effects, p.out_effects = EffectSequence.parse(r), EffectSequence.parse(r)
        p.frame_rate_code, p.default_selected, p.default_activated = r.u8(), r.u16(), r.u16()
        p.palette_id_ref = r.u8()
        for _ in range(r.u8()):
            bog = Bog(r.u16())
            bog.buttons = [Button.parse(r) for _ in range(r.u8())]
            p.bogs.append(bog)
        return p

    def pack(self) -> bytes:
        out = bytes([self.page_id, self.version]) + self.uo_mask
        out += self.in_effects.pack() + self.out_effects.pack()
        out += struct.pack(">BHHBB", self.frame_rate_code, self.default_selected,
                           self.default_activated, self.palette_id_ref, len(self.bogs))
        for g in self.bogs:
            out += struct.pack(">HB", g.default_valid_button, len(g.buttons))
            out += b"".join(b.pack() for b in g.buttons)
        return out


@dataclass
class Ics:
    video_width: int = 1920
    video_height: int = 1080
    frame_rate_byte: int = 0x60
    composition_number: int = 0
    composition_state: int = 0x80          # 0x80 = debut d'epoch
    sequence_descriptor: int = 0xC0
    stream_model: int = 0                  # 0 = multiplexe, 1 = hors multiplex
    ui_model: int = 0                      # 0 = normal, 1 = pop-up
    composition_timeout_pts: int = 0
    selection_timeout_pts: int = 0
    user_timeout_duration: int = 0
    pages: list = field(default_factory=list)

    @classmethod
    def parse(cls, b: bytes):
        r = Reader(b)
        i = cls(r.u16(), r.u16(), r.u8(), r.u16(), r.u8(), r.u8())
        length = int.from_bytes(r.raw(3), "big")
        if length != len(b) - 12:
            raise ValueError("longueur de composition incoherente")
        flags = r.u8()
        i.stream_model, i.ui_model = bits(flags, 1, 7), bits(flags, 1, 6)
        if i.stream_model == 0:
            i.composition_timeout_pts = int.from_bytes(r.raw(5), "big") & ((1 << 33) - 1)
            i.selection_timeout_pts = int.from_bytes(r.raw(5), "big") & ((1 << 33) - 1)
        i.user_timeout_duration = int.from_bytes(r.raw(3), "big")
        i.pages = [Page.parse(r) for _ in range(r.u8())]
        if r.pos != len(b):
            raise ValueError(f"{len(b) - r.pos} octets non consommes")
        return i

    def pack(self) -> bytes:
        body = bytes([(self.stream_model << 7) | (self.ui_model << 6)])
        if self.stream_model == 0:
            body += self.composition_timeout_pts.to_bytes(5, "big") + self.selection_timeout_pts.to_bytes(5, "big")
        body += self.user_timeout_duration.to_bytes(3, "big") + bytes([len(self.pages)])
        body += b"".join(p.pack() for p in self.pages)
        head = struct.pack(">HHBHBB", self.video_width, self.video_height, self.frame_rate_byte,
                           self.composition_number, self.composition_state, self.sequence_descriptor)
        return head + len(body).to_bytes(3, "big") + body
