"""Scan repository inputs for common credential formats without printing values.

This is a bounded format check, not an exhaustive secret or Git-history audit.
"""
import re
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
PATTERNS = [re.compile(pattern) for pattern in (
    rb"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----",
    rb"\bgh[pousr]_[A-Za-z0-9]{30,}\b",
    rb"\bgithub_pat_[A-Za-z0-9_]{40,}\b",
    rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
    rb"\b(?:cio|crc)_[A-Za-z0-9]{20,}\b",
    rb"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}\b",
)]


def main():
    files = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others",
                                     "--exclude-standard"], cwd=ROOT).split(b"\0")
    findings = []
    checked = 0
    for raw in sorted(set(files)):
        if not raw:
            continue
        path = ROOT / raw.decode("utf-8")
        if not path.is_file():
            continue
        data = path.read_bytes()
        checked += 1
        if any(pattern.search(data) for pattern in PATTERNS):
            findings.append(path.relative_to(ROOT).as_posix())
    for name in findings:
        print("Potential credential format:", name)
    print(f"Checked {checked} repository files; {len(findings)} files flagged; values redacted")
    return bool(findings)


if __name__ == "__main__":
    sys.exit(main())
