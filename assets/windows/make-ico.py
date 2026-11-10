"""Packs assets/icons/*.png into assets/windows/copperview.ico.

Each picture goes in as it is, PNG inside the ICO, which every Windows since
Vista reads. Only needed after the pictures change; the result is kept.
"""
import struct
from pathlib import Path

here = Path(__file__).resolve().parent
sizes = [16, 24, 32, 48, 64, 128, 256]
pictures = [(here.parent / "icons" / f"{s}.png").read_bytes() for s in sizes]

out = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
for size, data in zip(sizes, pictures):
    side = 0 if size >= 256 else size  # 0 means 256 in an ICO directory
    out += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
(here / "copperview.ico").write_bytes(out + b"".join(pictures))
