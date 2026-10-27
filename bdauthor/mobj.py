"""Parseur / ecriveur de MovieObject.bdmv (structure MOBJ) + desassembleur HDMV.

Le desassembleur est une premiere version, a comparer avec `mobj_dump`.
"""
import struct
from dataclasses import dataclass, field
from .reader import Reader, bits

GRP_BRANCH, GRP_CMP, GRP_SET = 0, 1, 2

BRANCH = {
    0: {0: "NOP", 1: "GOTO", 2: "BREAK"},
    1: {0: "JUMP_OBJECT", 1: "JUMP_TITLE", 2: "CALL_OBJECT", 3: "CALL_TITLE", 4: "RESUME"},
    2: {0: "PLAY_PL", 1: "PLAY_PL_PI", 2: "PLAY_PL_PM", 3: "TERMINATE_PL",
        4: "LINK_PI", 5: "LINK_MK"},
}
CMP = {1: "BC", 2: "EQ", 3: "NE", 4: "GE", 5: "GT", 6: "LE", 7: "LT"}
SET = {
    0: {1: "MOVE", 2: "SWAP", 3: "ADD", 4: "SUB", 5: "MUL", 6: "DIV", 7: "MOD", 8: "RND",
        9: "AND", 10: "OR", 11: "XOR", 12: "BITSET", 13: "BITCLR", 14: "SHL", 15: "SHR"},
    1: {1: "SET_STREAM", 2: "SET_NV_TIMER", 3: "SET_BUTTON_PAGE", 4: "ENABLE_BUTTON",
        5: "DISABLE_BUTTON", 6: "SET_SEC_STREAM", 7: "POPUP_OFF", 8: "STILL_ON",
        9: "STILL_OFF", 10: "SET_OUTPUT_MODE", 11: "SET_STREAM_SS"},
}


@dataclass
class Command:
    insn: int = 0     # mot de 32 bits (champs ci-dessous)
    dst: int = 0
    src: int = 0

    # --- champs de l'instruction ---
    op_cnt = property(lambda s: bits(s.insn, 3, 29))
    grp = property(lambda s: bits(s.insn, 2, 27))
    sub_grp = property(lambda s: bits(s.insn, 3, 24))
    imm_op1 = property(lambda s: bits(s.insn, 1, 23))
    imm_op2 = property(lambda s: bits(s.insn, 1, 22))
    branch_opt = property(lambda s: bits(s.insn, 4, 16))
    cmp_opt = property(lambda s: bits(s.insn, 4, 8))
    set_opt = property(lambda s: bits(s.insn, 5, 0))

    @classmethod
    def parse(cls, r: Reader):
        return cls(r.u32(), r.u32(), r.u32())

    def pack(self) -> bytes:
        return struct.pack(">III", self.insn, self.dst, self.src)

    @staticmethod
    def _reg(v):
        return f"PSR{v & 0x7fffffff}" if v & 0x80000000 else f"r{v}"

    def _operands(self):
        ops = []
        if self.op_cnt >= 1:
            ops.append(str(self.dst) if self.imm_op1 else self._reg(self.dst))
        if self.op_cnt >= 2:
            ops.append(str(self.src) if self.imm_op2 else self._reg(self.src))
        return ", ".join(ops)

    def text(self) -> str:
        g = self.grp
        if g == GRP_BRANCH:
            name = BRANCH.get(self.sub_grp, {}).get(self.branch_opt, f"BRANCH?{self.sub_grp}/{self.branch_opt}")
        elif g == GRP_CMP:
            name = "CMP_" + CMP.get(self.cmp_opt, f"?{self.cmp_opt}")
        elif g == GRP_SET:
            name = SET.get(self.sub_grp, {}).get(self.set_opt, f"SET?{self.sub_grp}/{self.set_opt}")
        else:
            name = f"GRP?{g}"
        return f"{name} {self._operands()}".rstrip()


@dataclass
class MovieObject:
    resume_intention: int = 0
    menu_call_mask: int = 0
    title_search_mask: int = 0
    commands: list = field(default_factory=list)


@dataclass
class MovieObjectBdmv:
    version: bytes = b"0200"
    objects: list = field(default_factory=list)

    @classmethod
    def parse(cls, data: bytes):
        r = Reader(data)
        if r.raw(4) != b"MOBJ":
            raise ValueError("signature MOBJ absente")
        m = cls(version=r.raw(4))
        r.seek(40)
        r.u32()               # longueur
        r.u32()               # reserve
        n = r.u16()
        for _ in range(n):
            flags = r.u16()
            ncmd = r.u16()
            m.objects.append(MovieObject(bits(flags, 1, 15), bits(flags, 1, 14),
                                         bits(flags, 1, 13),
                                         [Command.parse(r) for _ in range(ncmd)]))
        return m

    def pack(self) -> bytes:
        # corps = reserve(4) + nb_objets(2) + objets ; la longueur annoncee
        # (a l'offset 40) couvre tout ce qui suit le champ longueur
        body = bytes(4) + struct.pack(">H", len(self.objects))
        for o in self.objects:
            flags = (o.resume_intention << 15) | (o.menu_call_mask << 14) | (o.title_search_mask << 13)
            body += struct.pack(">HH", flags, len(o.commands)) + b"".join(c.pack() for c in o.commands)
        header = b"MOBJ" + self.version + bytes(32)       # 40 octets (extensions = 0)
        return header + struct.pack(">I", len(body)) + body

    def dump(self, only=None) -> str:
        out = [f"version {self.version.decode()}, {len(self.objects)} objets"]
        for i, o in enumerate(self.objects):
            if only is not None and i not in only:
                continue
            out.append(f"objet {i}: resume={o.resume_intention} menu_mask={o.menu_call_mask} "
                       f"title_mask={o.title_search_mask}")
            out += [f"  {j:3d}: {c.text()}" for j, c in enumerate(o.commands)]
        return "\n".join(out)
