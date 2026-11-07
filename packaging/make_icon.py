"""Rasterize assets/icon.svg's shapes to assets/icon.png (stdlib only)."""
import struct, sys, zlib

S = 256
PURPLE, WHITE = (0x6C, 0x3F, 0xB5), (255, 255, 255)

def in_rrect(x, y, x0, y0, w, h, r):
    cx = min(max(x, x0 + r), x0 + w - r)
    cy = min(max(y, y0 + r), y0 + h - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r

def color(x, y):
    c = None
    if in_rrect(x, y, 64, 8, 128, 240, 48): c = PURPLE
    for cx, cy, r, col in [(128, 96, 46, WHITE), (128, 96, 18, PURPLE),
                           (100, 176, 12, WHITE), (156, 176, 12, WHITE), (128, 212, 12, WHITE)]:
        if (x - cx) ** 2 + (y - cy) ** 2 <= r * r: c = col
    return c

rows = bytearray()
for y in range(S):
    rows.append(0)
    for x in range(S):
        # 4x4 supersampling for smooth edges
        acc, n = [0, 0, 0, 0], 0
        for sy in range(4):
            for sx in range(4):
                c = color(x + (sx + .5) / 4, y + (sy + .5) / 4)
                if c:
                    acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]; n += 1
        rows += bytes([acc[0] // n, acc[1] // n, acc[2] // n, n * 255 // 16] if n else [0, 0, 0, 0])

def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0)) \
    + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
