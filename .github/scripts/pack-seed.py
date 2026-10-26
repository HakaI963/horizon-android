#!/usr/bin/env python3
"""Package the JVM-derived part of a prepared pack for the Android launcher.

The client JAR is the player's (and the APK downloads it from Mojang); everything
under assets/, data/ and pack.png comes out of that jar on the phone. What stays
is the data the Java exporters and datagen produced, which no Rust-only launcher
can make. Those files, plus the pinned version JSON that names the client jar,
become one seed zip embedded in the APK.

Usage: pack-seed.py SEED.zip PREPARED_DIR MANIFEST.json
"""

from __future__ import annotations

import json
import sys
import zipfile
from pathlib import Path

SKIP_PREFIXES = ("assets/", "data/", "classes/", "client-classes/")
SKIP_FILES = {"pack.png"}


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    seed, prepared, manifest = (Path(arg).resolve() for arg in sys.argv[1:])
    if not (prepared / "horizon-resources.json").is_file():
        raise SystemExit(f"{prepared} has no horizon-resources.json")
    resources = json.loads((prepared / "horizon-resources.json").read_text())
    if resources.get("schema") != 7:
        raise SystemExit(f"unexpected resources schema {resources.get('schema')!r}")

    files = []
    for path in sorted(prepared.rglob("*")):
        if not path.is_file():
            continue
        name = path.relative_to(prepared).as_posix()
        if name in SKIP_FILES or name.endswith(".log"):
            continue
        if name.startswith(SKIP_PREFIXES):
            continue
        files.append((name, path))

    seed.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(seed, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        archive.write(manifest, "minecraft-manifest.json")
        for name, path in files:
            archive.write(path, name)
    raw = sum(path.stat().st_size for _, path in files) + manifest.stat().st_size
    print(
        f"seed {seed}: {len(files) + 1} files, "
        f"{raw / 1e6:.1f} MB raw, {seed.stat().st_size / 1e6:.1f} MB zipped"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
