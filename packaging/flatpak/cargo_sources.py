"""Write packaging/flatpak/cargo-sources.json from Cargo.lock (stdlib only).

Lists every crates.io crate so flatpak-builder can fetch them up front and
cargo can build offline. Same layout as flatpak-cargo-generator; run again
after Cargo.lock changes.
"""
import json, pathlib, tomllib

root = pathlib.Path(__file__).resolve().parents[2]
lock = tomllib.loads((root / "Cargo.lock").read_text())
sources = []
for pkg in lock["package"]:
    src = pkg.get("source")
    if src is None:
        continue  # the app itself
    if src != "registry+https://github.com/rust-lang/crates.io-index":
        raise SystemExit(f"{pkg['name']}: unsupported source {src}")
    name, version, sha = pkg["name"], pkg["version"], pkg["checksum"]
    dest = f"cargo/vendor/{name}-{version}"
    sources.append({
        "type": "archive",
        "archive-type": "tar-gzip",
        "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
        "sha256": sha,
        "dest": dest,
    })
    sources.append({
        "type": "inline",
        "contents": json.dumps({"package": sha, "files": {}}),
        "dest": dest,
        "dest-filename": ".cargo-checksum.json",
    })
sources.append({
    "type": "inline",
    "contents": '[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n[source.crates-io]\nreplace-with = "vendored-sources"\n',
    "dest": "cargo",
    "dest-filename": "config.toml",
})
out = root / "packaging/flatpak/cargo-sources.json"
out.write_text(json.dumps(sources, indent=4) + "\n")
print(f"{out.relative_to(root)}: {len(sources) // 2} crates")
