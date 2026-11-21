"""Regression checks for verify.sh exit handling, without compiling the workspace.

Run: python scripts/test_verify.py
"""
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest


class VerifyScriptTests(unittest.TestCase):
    def run_verify(self, stage="", suites=100, ignored=3):
        repo = Path(__file__).resolve().parent.parent
        bash = shutil.which("bash")
        git_bash = Path("C:/Program Files/Git/bin/bash.exe")
        if os.name == "nt" and git_bash.exists():
            bash = str(git_bash)
        if not bash:
            self.skipTest("Bash is required")
        with tempfile.TemporaryDirectory(prefix="spine-verify-") as tmp:
            cargo = Path(tmp) / "cargo"
            cargo.write_text("""#!/usr/bin/env bash
case "$1" in
test)
  i=0
  while [ "$i" -lt "$MOCK_SUITES" ]; do
    count=0
    if [ "$i" -eq 0 ]; then count="$MOCK_IGNORED"; fi
    echo "test result: ok. 1 passed; 0 failed; $count ignored; 0 measured; 0 filtered out; finished in 0s"
    i=$((i + 1))
  done
  ;;
esac
if [ "$1" = "$MOCK_FAIL" ]; then
  echo "terminated without a diagnostic keyword"
  exit 9
fi
exit 0
""", encoding="utf-8", newline="\n")
            cargo.chmod(0o755)
            path = Path(tmp).as_posix()
            if os.name == "nt":
                path = "/" + path[0].lower() + path[2:]
            env = dict(os.environ, MOCK_FAIL=stage, MOCK_SUITES=str(suites), MOCK_IGNORED=str(ignored))
            env.pop("SPINE_VERIFY_LOG_DIR", None)
            result = subprocess.run([bash, "-c", f"export PATH={shlex.quote(path)}:$PATH; bash scripts/verify.sh"],
                                    cwd=repo, env=env, text=True, capture_output=True, timeout=30)
            return result.returncode, result.stdout + result.stderr

    def test_complete_success(self):
        code, output = self.run_verify()
        self.assertEqual(code, 0, output)

    def test_each_failed_command_is_detected_without_keyword_matching(self):
        for stage in ("test", "clippy", "fmt"):
            with self.subTest(stage=stage):
                code, output = self.run_verify(stage=stage)
                self.assertEqual(code, 1, output)

    def test_truncated_run_is_rejected(self):
        code, output = self.run_verify(suites=1)
        self.assertEqual(code, 1, output)

    def test_missing_ignored_tests_is_rejected(self):
        code, output = self.run_verify(ignored=0)
        self.assertEqual(code, 1, output)


if __name__ == "__main__":
    unittest.main()
