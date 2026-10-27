"""Usage : python -m tests.roundtrip chemin/vers/index.bdmv"""
import sys
from bdauthor.index import IndexBdmv

data = open(sys.argv[1], "rb").read()
idx = IndexBdmv.parse(data)
print(idx.dump())
out = idx.pack()
print("\naller-retour identique :", out == data)
if out != data:
    for i, (a, b) in enumerate(zip(data, out)):
        if a != b:
            print(f"  1ere difference a l'offset {i}: {a:02x} != {b:02x}"); break
