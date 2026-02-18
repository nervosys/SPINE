"""Phase 19 doc updates: test counts 431→443, add Phase 19 entries."""
import re

files_to_update_counts = [
    "README.md",
    "ROADMAP.md",
    "OPTIMIZATIONS.md",
    "paper/paper.typ",
    ".github/copilot-instructions.md",
]

for fpath in files_to_update_counts:
    try:
        with open(fpath, "r", encoding="utf-8") as f:
            content = f.read()
        # Replace 431 test counts with 443
        updated = content.replace("431 tests", "443 tests")
        updated = updated.replace("431 passing", "443 passing")
        # Also update the test table count for spine-compiler: 9 → 21
        updated = updated.replace("| spine-compiler | 9 |", "| spine-compiler | 21 |")
        if updated != content:
            with open(fpath, "w", encoding="utf-8", newline="\n") as f:
                f.write(updated)
            print(f"  [OK] {fpath}")
        else:
            print(f"  [SKIP] {fpath} (no changes)")
    except FileNotFoundError:
        print(f"  [SKIP] {fpath} (not found)")

# Add Phase 19 to ROADMAP.md
roadmap = "ROADMAP.md"
with open(roadmap, "r", encoding="utf-8") as f:
    content = f.read()

phase19_entry = """### Phase 19: HLS Type System ✅

- [x] **Source location tracking**: `Span` type with line/column computation and merge
- [x] **Structured type errors**: `TypeError` with span, expected/found types, source-context formatting
- [x] **Error collection**: `TypeErrors` accumulator — reports ALL errors, not just first
- [x] **Multi-statement type checking**: `check_types_collect` handles Let, State, Assign, FnDef, Call, If, For, Element, Navigate, Search
- [x] **Function signature enforcement**: Param count, arg types, and return type checking
- [x] **Navigate/Search type checking**: Enforces string arguments
- [x] **Public type_check API**: `Compiler::type_check(source)` returns all errors at once
- [x] **443 tests passing**: +12 tests, 0 failures, 0 Clippy warnings

"""

# Insert Phase 19 after Phase 18 in the Completed section
if "Phase 19" not in content:
    # Find Phase 18 section end (next ### or ## after Phase 18)
    p18_match = re.search(r"### Phase 18.*?\n((?:- \[x\].*\n)*)", content)
    if p18_match:
        insert_pos = p18_match.end()
        content = content[:insert_pos] + "\n" + phase19_entry + content[insert_pos:]
        print("  [OK] Added Phase 19 to ROADMAP")
    else:
        print("  [WARN] Could not find Phase 18 in ROADMAP")

# Remove Phase 19 items from Planned section
content = re.sub(r"- \[ \] .*?Static type inference.*?\n", "", content)
content = re.sub(r"- \[ \] .*?Type-checked function signatures.*?\n", "", content)
content = re.sub(r"- \[ \] .*?Compile-time error reporting with source locations.*?\n", "", content)

with open(roadmap, "w", encoding="utf-8", newline="\n") as f:
    f.write(content)

# Update copilot-instructions.md with Phase 19 section
ci = ".github/copilot-instructions.md"
with open(ci, "r", encoding="utf-8") as f:
    content = f.read()

phase19_ci = """
### Phase 19: HLS Type System ✅

- [x] **Source location tracking**: `Span` type with line/column computation, merge, Display impl
- [x] **Structured type errors**: `TypeError` with span, expected/found types, source-context formatting (error[E0308] style)
- [x] **Error collection**: `TypeErrors` accumulator — reports ALL errors instead of aborting on first
- [x] **Multi-statement type checking**: `check_types_collect` for Let, State, Assign, FnDef, Call, If, For, Element, Navigate, Search
- [x] **Function signature enforcement**: Param count validation, arg type checking, return type mismatch detection
- [x] **Navigate/Search type guards**: Enforces string arguments with diagnostic
- [x] **Public `Compiler::type_check()` API**: Full source type-checking returning structured errors
- [x] **443 tests passing**: +12 tests (4 Span/TypeError + 8 type checking), 0 failures, 0 Clippy warnings
"""

if "Phase 19" not in content:
    p18_match = re.search(r"### Phase 18.*?\n((?:- \[x\].*\n)*)", content)
    if p18_match:
        insert_pos = p18_match.end()
        content = content[:insert_pos] + phase19_ci + content[insert_pos:]
        with open(ci, "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
        print("  [OK] Added Phase 19 to copilot-instructions.md")
    else:
        print("  [WARN] Could not find Phase 18 in copilot-instructions.md")
else:
    print("  [SKIP] Phase 19 already in copilot-instructions.md")

print("\nDone.")
