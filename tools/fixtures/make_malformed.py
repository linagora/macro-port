#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Generate malformed fixtures for Macro-Port robustness tests (ACCEPTANCE A2).

Usage: python3 -I make_malformed.py <generated_fixtures_dir> <out_dir>

Takes valid fixtures built by Build-Fixtures.ps1 (one .xlsm and one .xls are enough) and derives
broken variants. Pure standard library, deterministic (fixed seed). Writes malformed/expected.yaml.
Deep CFB nesting and invalid MS-OVBA compression chunks are better built in Rust unit tests,
where the structures can be crafted precisely.
"""
import io, os, random, sys, zipfile

FIXED = (1980, 1, 1, 0, 0, 0)


def zi(name):
    """ZipInfo with a fixed timestamp so that output bytes are reproducible."""
    info = zipfile.ZipInfo(name, date_time=FIXED)
    info.compress_type = zipfile.ZIP_DEFLATED
    return info

SEED = 20261018


def first(dir_, ext):
    for n in sorted(os.listdir(dir_)):
        if n.endswith("." + ext) and not n.startswith(("enc-", "xlm-", "none-")) and ".expected." not in n:
            return os.path.join(dir_, n)
    return None


def write(out, name, data, expected, rows):
    with open(os.path.join(out, name), "wb") as f:
        f.write(data)
    rows.append(f"  - file: malformed/{name}\n    expected_status: [{', '.join(expected)}]")


def main():
    src_dir, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    rnd = random.Random(SEED)
    rows = []

    # Format-independent cases
    write(out, "empty.xlsm", b"", ["corrupted", "unsupported"], rows)
    write(out, "random-bytes.xlsm", bytes(rnd.getrandbits(8) for _ in range(64 * 1024)), ["corrupted", "unsupported"], rows)
    write(out, "random-bytes.xls", bytes(rnd.getrandbits(8) for _ in range(64 * 1024)), ["corrupted", "unsupported"], rows)
    write(out, "text-renamed.xlsm", b"Sub NotAWorkbook()\nEnd Sub\n", ["corrupted", "unsupported"], rows)

    # Zip bomb: an xlsm-like zip whose vbaProject.bin inflates to 1 GiB of zeros (ratio far above 1000:1).
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        z.writestr(zi("[Content_Types].xml"), '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
        with z.open(zi("xl/vbaProject.bin"), "w", force_zip64=True) as f:
            chunk = b"\0" * (1 << 20)
            for _ in range(1024):
                f.write(chunk)
    write(out, "zip-bomb.xlsm", buf.getvalue(), ["suspicious_archive", "too_large"], rows)

    # Zip with a vbaProject.bin that is not a compound file
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(zi("[Content_Types].xml"), '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
        z.writestr(zi("xl/vbaProject.bin"), b"this is not a CFB file" * 100)
    write(out, "vbaproject-not-cfb.xlsm", buf.getvalue(), ["corrupted"], rows)

    # VBA project hidden in an .xlsx (should not contain macros): analysed, flagged sec.vbaproject-in-xlsx
    xlsm = first(src_dir, "xlsm")
    if xlsm:
        data = open(xlsm, "rb").read()
        write(out, "macros-in-xlsx.xlsx", data, ["ok"], rows)
        for frac, name in [(0.5, "truncated-half.xlsm"), (0.95, "truncated-tail.xlsm")]:
            write(out, name, data[: int(len(data) * frac)], ["corrupted", "error"], rows)
        # Corrupt the vbaProject.bin inside an otherwise valid zip
        zin = zipfile.ZipFile(io.BytesIO(data))
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
            for item in zin.infolist():
                b = zin.read(item.filename)
                if item.filename == "xl/vbaProject.bin":
                    b = bytearray(b)
                    for _ in range(64):
                        i = rnd.randrange(512, len(b))
                        b[i] = rnd.getrandbits(8)
                    b = bytes(b)
                z.writestr(item, b)
        write(out, "vbaproject-bitflips.xlsm", buf.getvalue(), ["ok", "corrupted", "error"], rows)
    else:
        print("warning: no valid .xlsm fixture found, zip-derived cases skipped", file=sys.stderr)

    xls = first(src_dir, "xls")
    if xls:
        data = bytearray(open(xls, "rb").read())
        bad_header = bytearray(data); bad_header[0:8] = b"\0" * 8
        write(out, "cfb-bad-signature.xls", bytes(bad_header), ["corrupted", "unsupported"], rows)
        bad_fat = bytearray(data)
        if len(bad_fat) > 0x4C + 4:
            bad_fat[0x4C:0x50] = b"\xff\xff\xff\x7f"  # first DIFAT entry points far outside the file
        write(out, "cfb-bad-fat.xls", bytes(bad_fat), ["corrupted"], rows)
        write(out, "cfb-truncated.xls", bytes(data[: len(data) // 3]), ["corrupted"], rows)
    else:
        print("warning: no valid .xls fixture found, CFB-derived cases skipped", file=sys.stderr)

    with open(os.path.join(out, "expected.yaml"), "w", encoding="utf-8") as f:
        f.write("# Generated by make_malformed.py: any listed status is acceptable, a panic or hang never is.\nfixtures:\n")
        f.write("\n".join(rows) + "\n")
    print(f"{len(rows)} malformed fixtures written to {out}")


if __name__ == "__main__":
    main()
