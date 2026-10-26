#!/usr/bin/env python3
"""Download the game files horizon-prepare imports, for CI only.

The preparer deliberately refuses to download Minecraft: a player owns a copy and
points it at their launcher's install. CI has no such install, so this script
builds the same layout from Mojang's public files, pinned by research-lock.json.
It never redistributes them; the seed the job produces contains only data derived
by running the preparer's exporters, and the APK fetches the client jar itself.

Usage: fetch-minecraft.py [DEST]     (default .ci)
Writes DEST/minecraft/versions/<v>/, DEST/minecraft/libraries/ and DEST/server.jar.
"""

from __future__ import annotations

import concurrent.futures
import hashlib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
LOCK = json.loads((ROOT / "research-lock.json").read_text())
GAME = LOCK["minecraft"]
VERSION = GAME["version"]
MANIFEST_SHA1 = GAME["manifest_sha1"]
CLIENT_SHA1 = GAME["client_sha1"]
SERVER_SHA1 = GAME["server_sha1"]
MANIFEST_URL = (
    f"https://piston-meta.mojang.com/v1/packages/{MANIFEST_SHA1}/{VERSION}.json"
)
ALLOWED_HOSTS = (".mojang.com", ".minecraft.net")
CHUNK = 1 << 20


def allowed_url(url: str) -> bool:
    host = urllib.parse.urlsplit(url).hostname or ""
    return url.startswith("https://") and any(
        host == suffix[1:] or host.endswith(suffix) for suffix in ALLOWED_HOSTS
    )


def fetch(url: str, destination: Path, sha1: str, size: int | None = None) -> int:
    if not allowed_url(url):
        raise RuntimeError(f"refusing a download outside Mojang: {url}")
    if destination.is_file() and hashlib.sha1(destination.read_bytes()).hexdigest() == sha1:
        return destination.stat().st_size
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_name(destination.name + ".part")
    for attempt in range(3):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "horizon-ci"})
            digest = hashlib.sha1()
            written = 0
            with urllib.request.urlopen(request, timeout=60) as response, open(
                partial, "wb"
            ) as out:
                while True:
                    block = response.read(CHUNK)
                    if not block:
                        break
                    digest.update(block)
                    out.write(block)
                    written += len(block)
            if digest.hexdigest() != sha1:
                raise RuntimeError(
                    f"{url}: sha1 {digest.hexdigest()}, expected {sha1}"
                )
            if size is not None and written != size:
                raise RuntimeError(f"{url}: {written} bytes, expected {size}")
            os.replace(partial, destination)
            return written
        except (urllib.error.URLError, OSError, RuntimeError) as error:
            partial.unlink(missing_ok=True)
            if attempt == 2:
                raise
            print(f"retrying {url} after {error}", file=sys.stderr)
            time.sleep(1 + attempt * 2)
    raise AssertionError("unreachable")


def allowed_library(library: dict) -> bool:
    rules = library.get("rules")
    if not rules:
        return True
    result = False
    for rule in rules:
        system = rule.get("os") or {}
        name = system.get("name")
        if name is not None and name != "linux":
            continue
        arch = system.get("arch")
        if arch is not None and not any(
            re.fullmatch(arch, candidate) for candidate in ("x86_64", "amd64", "x64")
        ):
            continue
        # The exporters use no optional features; a rule that asks for one is not
        # a rule for this run.
        if rule.get("features"):
            continue
        result = rule.get("action") == "allow"
    return result


def safe_path(path: str) -> Path:
    posix = PurePosixPath(path)
    if posix.is_absolute() or ".." in posix.parts or not posix.parts:
        raise RuntimeError(f"unsafe path in the version JSON: {path!r}")
    return Path(*posix.parts)


def main() -> int:
    destination = Path(sys.argv[1] if len(sys.argv) > 1 else ".ci").resolve()
    minecraft = destination / "minecraft"
    versions = minecraft / "versions" / VERSION
    libraries = minecraft / "libraries"
    versions.mkdir(parents=True, exist_ok=True)
    libraries.mkdir(parents=True, exist_ok=True)

    manifest_path = versions / f"{VERSION}.json"
    fetch(MANIFEST_URL, manifest_path, MANIFEST_SHA1)
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("id") != VERSION:
        raise RuntimeError(f"{manifest_path} is {manifest.get('id')!r}, not {VERSION}")

    client = manifest["downloads"]["client"]
    server = manifest["downloads"]["server"]
    if client["sha1"] != CLIENT_SHA1 or server["sha1"] != SERVER_SHA1:
        raise RuntimeError("the version JSON does not name the jars research-lock pins")

    downloads: list[tuple[str, Path, str, int | None]] = [
        (client["url"], versions / f"{VERSION}.jar", CLIENT_SHA1, client.get("size")),
        (server["url"], destination / "server.jar", SERVER_SHA1, server.get("size")),
    ]
    for library in manifest.get("libraries", []):
        artifact = (library.get("downloads") or {}).get("artifact")
        if not artifact or not allowed_library(library):
            continue
        path = safe_path(artifact["path"])
        downloads.append(
            (artifact["url"], libraries / path, artifact["sha1"], artifact.get("size"))
        )

    total = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        futures = {
            pool.submit(fetch, url, path, sha1, size): path
            for url, path, sha1, size in downloads
        }
        for future in concurrent.futures.as_completed(futures):
            path = futures[future]
            total += future.result()
            print(f"  {path.relative_to(destination)}")

    print(
        f"Minecraft {VERSION}: {len(downloads)} files, {total / 1e6:.1f} MB under {destination}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
