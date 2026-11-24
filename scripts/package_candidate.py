"""Build a local native candidate; never publish or include runtime secrets.

Usage: python scripts/package_candidate.py [--no-build]
Produces a deterministic ZIP, member checksums, dependency inventory, and source
provenance. A dirty checkout is identified as an unreviewed development candidate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    return subprocess.check_output(args, cwd=ROOT)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    if not args.no_build:
        subprocess.run(["cargo", "build", "--locked", "--release", "-p", "spine-core",
                        "-p", "spine-cli", "-p", "spine-gateway"], cwd=ROOT, check=True)
    metadata = json.loads(run("cargo", "metadata", "--locked", "--format-version", "1"))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "spine-core")
    rustc = run("rustc", "-vV").decode().strip()
    target = next(line.split(": ", 1)[1] for line in rustc.splitlines() if line.startswith("host:"))
    commit = run("git", "rev-parse", "HEAD").decode().strip()
    status = run("git", "status", "--porcelain").decode().strip()
    entries = {}
    suffix = ".exe" if os.name == "nt" else ""
    for name in ("spine", "spine-core", "spine-gateway"):
        entries["bin/" + name + suffix] = (Path(metadata["target_directory"]) / "release" / (name + suffix)).read_bytes()
    for name in ("LICENSE", "Cargo.lock", "SECURITY.md", "docs/AGENT_WEB_QUICKSTART.md",
                 "docs/COMPLETION_STATUS.md", "docs/NATIVE_OPERATIONS.md", "docs/CRYPTO_REVIEW_PACKET.md",
                 "docs/DEPENDENCY_EXCEPTIONS.md"):
        entries[name] = (ROOT / name).read_bytes()
    for path in sorted((ROOT / "examples/config").glob("*.toml")):
        entries["examples/" + path.name] = path.read_bytes()
    inventory = [{"name": p["name"], "version": p["version"], "license": p["license"],
                  "source": p["source"]} for p in metadata["packages"]]
    entries["dependencies.json"] = json.dumps(inventory, indent=2, sort_keys=True).encode()
    # Hash actual repository inputs, including untracked source; do not archive them.
    sources = run("git", "ls-files", "-z", "--cached", "--others", "--exclude-standard").split(b"\0")
    source_hash = hashlib.sha256()
    for raw in sorted(set(sources)):
        if not raw:
            continue
        path = ROOT / os.fsdecode(raw)
        if path.is_file():
            source_hash.update(raw + b"\0" + hashlib.sha256(path.read_bytes()).digest())
    entries["provenance.json"] = json.dumps({"version": version, "target": target,
        "base_commit": commit, "dirty_checkout": bool(status), "source_digest_sha256": source_hash.hexdigest(),
        "rustc": rustc, "status": "local development candidate; no publication or independent security review",
        "build_command": "cargo build --locked --release -p spine-core -p spine-cli -p spine-gateway"},
        indent=2, sort_keys=True).encode()
    entries["SHA256SUMS"] = "".join(f"{hashlib.sha256(data).hexdigest()}  {name}\n"
                                      for name, data in sorted(entries.items())).encode()
    output = ROOT / "artifacts"
    output.mkdir(exist_ok=True)
    archive = output / f"spine-{version}-dev-{target}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for name, data in sorted(entries.items()):
            entry = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = (0o755 if name.startswith("bin/") else 0o644) << 16
            bundle.writestr(entry, data)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(archive)


if __name__ == "__main__":
    main()
