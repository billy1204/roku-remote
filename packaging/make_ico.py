"""Wrap assets/icon.png in a single-image .ico (PNG-in-ICO, Windows Vista+)."""
import struct, sys
png = open(sys.argv[1], "rb").read()
# ICONDIR, then one ICONDIRENTRY: 0 width/height means 256.
ico = struct.pack("<HHH", 0, 1, 1) + struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 6 + 16) + png
open(sys.argv[2], "wb").write(ico)
