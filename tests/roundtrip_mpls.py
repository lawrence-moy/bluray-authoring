"""Usage : python -m tests.roundtrip_mpls 00000.mpls"""
import sys
from bdauthor.mpls import Mpls

data = open(sys.argv[1], "rb").read()
m = Mpls.parse(data)
print(m.dump())
out = m.pack()
print("\naller-retour identique :", out == data, f"({len(out)} / {len(data)} octets)")
