"""Usage : python -m tests.roundtrip_clpi 00000.clpi [autre.clpi ...]"""
import sys
from bdauthor.clpi import Clpi

for path in sys.argv[1:]:
    data = open(path, "rb").read()
    c = Clpi.parse(data)
    print(f"=== {path}")
    print(c.dump())
    out = c.pack()
    print("aller-retour identique :", out == data, f"({len(out)} / {len(data)} octets)\n")
    if out != data:
        for i, (a, b) in enumerate(zip(data, out)):
            if a != b:
                print(f"  1ere difference a l'offset {i:#x}: {a:02x} != {b:02x}"); break
