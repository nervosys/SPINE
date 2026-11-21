"""Run local candidate gates in order; stop at the first failed command.

Logs and candidate artifacts stay local. This script never publishes or tags.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    bash = Path("C:/Program Files/Git/bin/bash.exe") if os.name == "nt" else Path(shutil.which("bash") or "bash")
    env = dict(os.environ, SPINE_VERIFY_LOG_DIR="build_completion_gate_checks")
    # The exact passed-test total is not hard-coded here. The 1478 this script
    # used to pass was measured on the pre-v1 wire (it predates the native wire
    # migration, which deleted and added tests), so it no longer holds. Set
    # SPINE_EXPECTED_TESTS to the total from a fresh full run once one exists;
    # until then verify.sh still checks failures, suite count and ignored count.
    expected_total = [os.environ["SPINE_EXPECTED_TESTS"]] if os.environ.get("SPINE_EXPECTED_TESTS") else []
    steps = [
        ("final_verified", [str(bash), "scripts/verify.sh", *expected_total]),
        ("script_regressions", [sys.executable, "scripts/test_verify.py"]),
        ("final_audit", ["cargo", "audit", "--json"]),
        ("final_deny", ["cargo", "deny", "check", "advisories", "licenses"]),
        ("final_secrets", [sys.executable, "scripts/scan_secrets.py"]),
        ("final_msrv", ["cargo", "+1.89", "check", "--workspace", "--locked"]),
        ("release", ["cargo", "build", "--locked", "--release", "-p", "spine-core", "-p", "spine-cli", "-p", "spine-gateway"]),
        ("candidate_package", [sys.executable, "scripts/package_candidate.py", "--no-build"]),
    ]
    for name, command in steps:
        print("Running", name, flush=True)
        with (ROOT / f"build_completion_gate_{name}.log").open("w", encoding="utf-8") as log:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        if result.returncode:
            print(f"FAILED: {name}; see build_completion_gate_{name}.log", flush=True)
            return result.returncode
    # Choose the path emitted by this run, not a stale archive for another target.
    archive = (ROOT / "build_completion_gate_candidate_package.log").read_text().strip().splitlines()[-1]
    with (ROOT / "build_completion_candidate_smoke.log").open("w", encoding="utf-8") as log:
        result = subprocess.run([sys.executable, "scripts/smoke_candidate.py", archive], cwd=ROOT,
                                env=env, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        print("FAILED: clean candidate installation; see build_completion_candidate_smoke.log", flush=True)
        return result.returncode
    print("All local candidate gates passed; publication and other-platform acceptance remain separate.", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
