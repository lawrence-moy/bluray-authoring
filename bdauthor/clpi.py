"""Parseur / ecriveur de fichiers .clpi (informations de clip, signature HDMV).

Sections : ClipInfo, SequenceInfo, ProgramInfo, CPI (EP_map), ClipMark.
Les attributs de flux (ProgramInfo) et le contenu de ClipMark sont gardes bruts.
"""
import struct
from dataclasses import dataclass, field
from .reader import Reader, bits

CODING = {0x01: "MPEG-1 video", 0x02: "MPEG-2 video", 0x1B: "H.264", 0x24: "HEVC", 0xEA: "VC-1",
          0x03: "MPEG-1 audio", 0x04: "MPEG-2 audio", 0x80: "LPCM", 0x81: "AC3", 0x82: "DTS",
          0x83: "TrueHD", 0x84: "E-AC3", 0x85: "DTS-HD", 0x86: "DTS-HD MA",
          0x90: "PG (sous-titres)", 0x91: "IG (menu)", 0x92: "Text sous-titres"}
VIDEO_FMT = {1: "480i", 2: "576i", 3: "480p", 4: "1080i", 5: "720p", 6: "1080p", 7: "576p"}
VIDEO_RATE = {1: "23.976", 2: "24", 3: "25", 4: "29.97", 6: "50", 7: "59.94"}
APP_TYPES = {1: "film (main TS)", 2: "diaporama", 3: "diaporama navigable", 4: "sous-TS diaporama",
             5: "sous-TS menu interactif", 6: "sous-TS texte", 7: "sous-chemin (ES)"}


@dataclass
class ClipInfo:
    clip_stream_type: int = 1
    application_type: int = 1
    is_cc5: int = 0
    ts_recording_rate: int = 0        # octets / seconde
    num_source_packets: int = 0       # paquets de 192 octets
    ts_validity: int = 0x80
    ts_format_id: bytes = b"HDMV"

    @classmethod
    def parse(cls, r: Reader):
        r.u32(); r.u16()
        c = cls(clip_stream_type=r.u8(), application_type=r.u8())
        c.is_cc5 = bits(r.u32(), 1, 0)
        c.ts_recording_rate, c.num_source_packets = r.u32(), r.u32()
        r.raw(128)
        r.u16()
        c.ts_validity = r.u8()
        c.ts_format_id = r.raw(4)
        r.raw(25)
        return c

    def pack(self) -> bytes:
        ts = struct.pack(">B", self.ts_validity) + self.ts_format_id + bytes(25)
        body = (struct.pack(">HBBIII", 0, self.clip_stream_type, self.application_type,
                            self.is_cc5, self.ts_recording_rate, self.num_source_packets)
                + bytes(128) + struct.pack(">H", len(ts)) + ts)
        return struct.pack(">I", len(body)) + body


@dataclass
class StcSeq:
    pcr_pid: int = 0
    spn_stc_start: int = 0
    start_time: int = 0               # horloge 45 kHz
    end_time: int = 0


@dataclass
class AtcSeq:
    spn_atc_start: int = 0
    offset_stc_id: int = 0
    stc: list = field(default_factory=list)


@dataclass
class SequenceInfo:
    atc: list = field(default_factory=list)

    @classmethod
    def parse(cls, r: Reader):
        r.u32(); r.u8()
        s = cls()
        for _ in range(r.u8()):
            a = AtcSeq(spn_atc_start=r.u32())
            n = r.u8()
            a.offset_stc_id = r.u8()
            a.stc = [StcSeq(r.u16(), r.u32(), r.u32(), r.u32()) for _ in range(n)]
            s.atc.append(a)
        return s

    def pack(self) -> bytes:
        body = struct.pack(">BB", 0, len(self.atc))
        for a in self.atc:
            body += struct.pack(">IBB", a.spn_atc_start, len(a.stc), a.offset_stc_id)
            body += b"".join(struct.pack(">HIII", t.pcr_pid, t.spn_stc_start, t.start_time, t.end_time)
                             for t in a.stc)
        return struct.pack(">I", len(body)) + body


@dataclass
class Stream:
    pid: int = 0
    coding_type: int = 0
    attrs: bytes = b""                # octets d'attributs bruts (apres coding_type)


@dataclass
class Program:
    spn_start: int = 0
    pmt_pid: int = 0
    num_groups: int = 0
    streams: list = field(default_factory=list)


@dataclass
class ProgramInfo:
    programs: list = field(default_factory=list)

    @classmethod
    def parse(cls, r: Reader):
        r.u32(); r.u8()
        info = cls()
        for _ in range(r.u8()):
            p = Program(spn_start=r.u32(), pmt_pid=r.u16())
            n = r.u8()
            p.num_groups = r.u8()
            for _ in range(n):
                pid, ln = r.u16(), r.u8()
                p.streams.append(Stream(pid, r.u8(), r.raw(ln - 1)))
            info.programs.append(p)
        return info

    def pack(self) -> bytes:
        body = struct.pack(">BB", 0, len(self.programs))
        for p in self.programs:
            body += struct.pack(">IHBB", p.spn_start, p.pmt_pid, len(p.streams), p.num_groups)
            for s in p.streams:
                body += struct.pack(">HBB", s.pid, len(s.attrs) + 1, s.coding_type) + s.attrs
        return struct.pack(">I", len(body)) + body


@dataclass
class EpCoarse:
    ref_fine: int = 0                 # 18 bits : index de l'entree fine associee
    pts: int = 0                      # 14 bits : PTS[32:19]
    spn: int = 0


@dataclass
class EpFine:
    angle_change: int = 0             # 1 bit
    end_offset: int = 0               # 3 bits
    pts: int = 0                      # 11 bits : PTS[19:9]
    spn: int = 0                      # 17 bits : poids faibles du SPN


@dataclass
class EpStream:
    pid: int = 0
    ep_stream_type: int = 1
    coarse: list = field(default_factory=list)
    fine: list = field(default_factory=list)

    def entries(self):
        """(pts 90 kHz, spn complet) de chaque point d'acces."""
        out, ci = [], 0
        for i, f in enumerate(self.fine):
            while ci + 1 < len(self.coarse) and self.coarse[ci + 1].ref_fine <= i:
                ci += 1
            c = self.coarse[ci]
            out.append(((c.pts << 19) | (f.pts << 9), (c.spn & ~0x1FFFF) | f.spn))
        return out


@dataclass
class Cpi:
    cpi_type: int = 1
    streams: list = field(default_factory=list)

    @classmethod
    def parse(cls, r: Reader):
        length = r.u32()
        if length == 0:
            return None
        c = cls(cpi_type=bits(r.u16(), 4, 0))
        base = r.pos                          # debut de l'EP_map
        r.u8()
        n = r.u8()
        heads = []
        for _ in range(n):
            pid, v, addr = r.u16(), int.from_bytes(r.raw(4), "big"), r.u32()
            heads.append((pid, v, addr))
        # champ de 10 octets = reserve(10) type(4) coarse(16) fine(18) addr(32)
        r.seek(base + 2)
        for _ in range(n):
            pid = r.u16()
            v = int.from_bytes(r.raw(10), "big")
            addr = v & 0xFFFFFFFF
            nfine, ncoarse = bits(v, 18, 32), bits(v, 16, 50)
            s = EpStream(pid=pid, ep_stream_type=bits(v, 4, 66))
            save = r.pos
            r.seek(base + addr)
            start = r.pos
            fine_off = r.u32()
            for _ in range(ncoarse):
                w = r.u32()
                s.coarse.append(EpCoarse(bits(w, 18, 14), bits(w, 14, 0), r.u32()))
            r.seek(start + fine_off)
            for _ in range(nfine):
                w = r.u32()
                s.fine.append(EpFine(bits(w, 1, 31), bits(w, 3, 28), bits(w, 11, 17), bits(w, 17, 0)))
            c.streams.append(s)
            r.seek(save)
        return c

    def pack(self) -> bytes:
        n = len(self.streams)
        blocks, heads, addr = [], b"", 2 + 12 * n
        for s in self.streams:
            blk = struct.pack(">I", 4 + 8 * len(s.coarse))
            blk += b"".join(struct.pack(">II", (c.ref_fine << 14) | c.pts, c.spn) for c in s.coarse)
            blk += b"".join(struct.pack(">I", (f.angle_change << 31) | (f.end_offset << 28)
                                        | (f.pts << 17) | f.spn) for f in s.fine)
            v = ((s.ep_stream_type << 66) | (len(s.coarse) << 50) | (len(s.fine) << 32) | addr)
            heads += struct.pack(">H", s.pid) + v.to_bytes(10, "big")
            blocks.append(blk)
            addr += len(blk)
        body = struct.pack(">H", self.cpi_type) + bytes([0, n]) + heads + b"".join(blocks)
        return struct.pack(">I", len(body)) + body


@dataclass
class Clpi:
    version: bytes = b"0200"
    clip_info: ClipInfo = field(default_factory=ClipInfo)
    sequence: SequenceInfo = field(default_factory=SequenceInfo)
    program: ProgramInfo = field(default_factory=ProgramInfo)
    cpi: object = None                # Cpi ou None (clip sans EP_map, ex. menu IG)
    clip_mark: bytes = b"\0\0\0\0"    # section brute, longueur incluse
    extension: bytes = b""

    @classmethod
    def parse(cls, data: bytes):
        r = Reader(data)
        if r.raw(4) != b"HDMV":
            raise ValueError("signature HDMV absente")
        c = cls(version=r.raw(4))
        seq, prog, cpi, mark, ext = (r.u32() for _ in range(5))
        r.seek(40);   c.clip_info = ClipInfo.parse(r)
        r.seek(seq);  c.sequence = SequenceInfo.parse(r)
        r.seek(prog); c.program = ProgramInfo.parse(r)
        r.seek(cpi);  c.cpi = Cpi.parse(r)
        r.seek(mark)
        c.clip_mark = data[mark:mark + 4 + r.u32()]
        if ext:
            c.extension = data[ext:]
        return c

    def pack(self) -> bytes:
        secs = [self.clip_info.pack(), self.sequence.pack(), self.program.pack(),
                self.cpi.pack() if self.cpi else bytes(4), self.clip_mark]
        pos = [40]
        for s in secs:
            pos.append(pos[-1] + len(s))
        ext = pos[-1] if self.extension else 0
        head = (b"HDMV" + self.version
                + struct.pack(">IIIII", pos[1], pos[2], pos[3], pos[4], ext) + bytes(12))
        return head + b"".join(secs) + self.extension

    def dump(self) -> str:
        ci = self.clip_info
        out = [f"version {self.version.decode()}",
               f"ClipInfo : type d'application {ci.application_type} = {APP_TYPES.get(ci.application_type, '?')}, "
               f"{ci.num_source_packets} paquets ({ci.num_source_packets * 192} octets), "
               f"debit {ci.ts_recording_rate} o/s"]
        for a in self.sequence.atc:
            for t in a.stc:
                out.append(f"Sequence : PCR PID 0x{t.pcr_pid:04x}, "
                           f"{t.start_time / 45000:.3f}s -> {t.end_time / 45000:.3f}s")
        for p in self.program.programs:
            out.append(f"Programme : PMT PID 0x{p.pmt_pid:04x}")
            for s in p.streams:
                extra = ""
                if s.coding_type in (0x01, 0x02, 0x1B, 0x24, 0xEA) and s.attrs:
                    extra = f" {VIDEO_FMT.get(s.attrs[0] >> 4, '?')} {VIDEO_RATE.get(s.attrs[0] & 15, '?')} i/s"
                elif s.coding_type in (0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86) and len(s.attrs) >= 4:
                    extra = f" langue {s.attrs[1:4].decode('ascii', 'replace')}"
                elif s.coding_type in (0x90, 0x91) and len(s.attrs) >= 3:
                    extra = f" langue {s.attrs[0:3].decode('ascii', 'replace')}"
                out.append(f"  flux PID 0x{s.pid:04x} : {CODING.get(s.coding_type, hex(s.coding_type))}{extra}")
        if self.cpi is None:
            out.append("EP_map : aucune")
        for s in (self.cpi.streams if self.cpi else []):
            e = s.entries()
            out.append(f"EP_map PID 0x{s.pid:04x} : {len(s.coarse)} entrees coarse, {len(e)} fine")
            for pts, spn in e[:4]:
                out.append(f"  t={pts / 90000:.3f}s  SPN={spn}  (octet {spn * 192})")
            if len(e) > 4:
                out.append(f"  ... derniere : t={e[-1][0] / 90000:.3f}s  SPN={e[-1][1]}")
        return "\n".join(out)
