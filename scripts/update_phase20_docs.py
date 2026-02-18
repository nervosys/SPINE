"""Update docs for Phase 20: Agent Ontology System."""
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
        updated = content.replace("443 tests", "458 tests")
        updated = updated.replace("443 passing", "458 passing")
        # Update spine-agentic test count: 4 → 19 (was 5 before, but table might say 4)
        updated = updated.replace("| spine-agentic | 4 |", "| spine-agentic | 19 |")
        updated = updated.replace("| spine-agentic | 5 |", "| spine-agentic | 19 |")
        if updated != content:
            with open(fpath, "w", encoding="utf-8", newline="\n") as f:
                f.write(updated)
            print(f"  [OK] {fpath}")
        else:
            print(f"  [SKIP] {fpath} (no changes)")
    except FileNotFoundError:
        print(f"  [SKIP] {fpath} (not found)")

# Add Phase 20 to ROADMAP.md
roadmap = "ROADMAP.md"
with open(roadmap, "r", encoding="utf-8") as f:
    content = f.read()

phase20_entry = """### Phase 20: Agent Ontology System ✅

- [x] **OntologyTerm**: URI-based terms with labels, descriptions, parent hierarchy, properties
- [x] **AgentOntology**: Namespace-versioned ontology with term management and whole-ontology hashing
- [x] **Cryptographic hashes**: SHA-256 per-term and whole-ontology hashes for HashOnly visibility
- [x] **Neural hashes**: Locality-sensitive embeddings for NeuralHash visibility (approximate matching)
- [x] **Visibility controls**: Public, HashOnly, NeuralHash, Private per-term visibility
- [x] **DisclosedOntology**: Privacy-preserving views combining cleartext, hashed, and neural terms
- [x] **OntologyAccessControl**: Per-agent permission rules with first-match-wins resolution
- [x] **OntologyRegistry**: Discovery index with term lookup, hash verification, and neural similarity search
- [x] **AgentProfile integration**: `ontology` field with `with_ontology()` builder
- [x] **Compatibility scoring**: Jaccard similarity between agents' public ontology terms
- [x] **458 tests passing**: +15 tests (14 ontology + 1 agentic), 0 failures, 0 Clippy warnings

"""

if "Phase 20" not in content:
    p19_match = re.search(r"### Phase 19.*?\n((?:- \[x\].*\n)*)", content)
    if p19_match:
        insert_pos = p19_match.end()
        content = content[:insert_pos] + "\n" + phase20_entry + content[insert_pos:]
        print("  [OK] Added Phase 20 to ROADMAP")
    else:
        print("  [WARN] Could not find Phase 19 in ROADMAP")

with open(roadmap, "w", encoding="utf-8", newline="\n") as f:
    f.write(content)

# Update copilot-instructions.md
ci = ".github/copilot-instructions.md"
with open(ci, "r", encoding="utf-8") as f:
    content = f.read()

phase20_ci = """
### Phase 20: Agent Ontology System ✅

- [x] **OntologyTerm**: URI-based terms with labels, descriptions, parent hierarchy, properties
- [x] **AgentOntology**: Namespace-versioned ontology with term management and whole-ontology hashing
- [x] **Cryptographic hashes**: SHA-256 per-term and whole-ontology hashes for HashOnly visibility
- [x] **Neural hashes**: Locality-sensitive embeddings for NeuralHash visibility (approximate matching)
- [x] **Visibility controls**: Public, HashOnly, NeuralHash, Private per-term visibility
- [x] **DisclosedOntology**: Privacy-preserving views combining cleartext, hashed, and neural terms
- [x] **OntologyAccessControl**: Per-agent permission rules with first-match-wins resolution
- [x] **OntologyRegistry**: Discovery index with term lookup, hash verification, and neural similarity search
- [x] **AgentProfile integration**: `ontology` field with `with_ontology()` builder
- [x] **Compatibility scoring**: Jaccard similarity between agents' public ontology terms
- [x] **458 tests passing**: +15 tests (14 ontology + 1 agentic), 0 failures, 0 Clippy warnings
"""

if "Phase 20" not in content:
    p19_match = re.search(r"### Phase 19.*?\n((?:- \[x\].*\n)*)", content)
    if p19_match:
        insert_pos = p19_match.end()
        content = content[:insert_pos] + phase20_ci + content[insert_pos:]
        with open(ci, "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
        print("  [OK] Added Phase 20 to copilot-instructions.md")
    else:
        print("  [WARN] Could not find Phase 19 in copilot-instructions.md")
else:
    print("  [SKIP] Phase 20 already in copilot-instructions.md")

print("\nDone.")
