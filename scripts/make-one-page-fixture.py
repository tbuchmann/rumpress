#!/usr/bin/env python3
"""Generate tests/fixtures/one-page.pdf: a minimal 1-page PDF with no
external dependencies. The page is 612x792 pt (US Letter), i.e. NOT a
2x1 beamer page, so it is distinguishable from the two-page fixture."""

import zlib

PAGE_W, PAGE_H = 612, 792


def content_stream(w, h, r, g, b):
    text = f"Page {w}x{h}"
    ops = (
        f"{r} {g} {b} rg\n"
        f"0 0 {w} {h} re f\n"
        f"1 1 1 rg\n"
        f"BT /F1 48 Tf 72 {h - 120} Td ({text}) Tj ET\n"
    )
    return ops.encode("ascii")


def build_pdf():
    objects = []  # (num, bytes)

    objects.append((1, b"<< /Type /Catalog /Pages 2 0 R >>"))
    objects.append((2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"))
    objects.append(
        (
            3,
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] "
            f"/Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R >>".encode(),
        )
    )
    objects.append((5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"))

    stream = content_stream(PAGE_W, PAGE_H, 0.3, 0.7, 0.3)
    objects.append(
        (
            7,
            b"<< /Length "
            + str(len(stream)).encode()
            + b" >>\nstream\n"
            + stream
            + b"endstream",
        )
    )

    # Renumber objects densely 1..N so the xref table is contiguous.
    objects.sort(key=lambda item: item[0])
    remap = {old: new for new, (old, _) in enumerate(objects, start=1)}
    objects = [(remap[old], body) for old, body in objects]
    for old, new in remap.items():
        if old != new:
            objects = [
                (num, body.replace(f"{old} 0 R".encode(), f"{new} 0 R".encode()))
                for num, body in objects
            ]

    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"

    xref_pos = len(out)
    count = max(offsets) + 1
    out += f"xref\n0 {count}\n".encode()
    out += b"0000000000 65535 f \n"
    for num in range(1, count):
        out += f"{offsets[num]:010d} 00000 n \n".encode()
    out += (
        f"trailer\n<< /Size {count} /Root 1 0 R >>\n"
        f"startxref\n{xref_pos}\n%%EOF\n"
    ).encode()
    return bytes(out)


if __name__ == "__main__":
    import pathlib

    path = pathlib.Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "one-page.pdf"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(build_pdf())
    print(f"wrote {path} ({path.stat().st_size} bytes)")
