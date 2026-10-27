"""Lecture binaire big-endian avec champs de quelques bits."""
import struct


class Reader:
    def __init__(self, data: bytes, pos: int = 0):
        self.data, self.pos = data, pos

    def u8(self):  return self._unpack(">B", 1)
    def u16(self): return self._unpack(">H", 2)
    def u32(self): return self._unpack(">I", 4)

    def raw(self, n: int) -> bytes:
        b = self.data[self.pos:self.pos + n]
        if len(b) != n:
            raise EOFError(f"lecture de {n} octets impossible a l'offset {self.pos}")
        self.pos += n
        return b

    def seek(self, pos: int):
        self.pos = pos

    def _unpack(self, fmt, n):
        return struct.unpack(fmt, self.raw(n))[0]


def bits(value: int, width: int, shift: int) -> int:
    """Extrait `width` bits de `value`, a partir du bit `shift` (0 = poids faible)."""
    return (value >> shift) & ((1 << width) - 1)
