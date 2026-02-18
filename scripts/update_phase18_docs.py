"""Phase 18 doc updates: test counts + ROADMAP + copilot-instructions entries."""
import re

OLD_COUNT = "429"
NEW_COUNT = "431"

# Files to update test counts (skip historical entries)
count_files = {
    "README.md": True,
    "ROADMAP.md": False,  # has historical entries, handle separately
    "OPTIMIZATIONS.md": True,
    "paper/paper.typ": True,
}

for path, simple in count_files.items():
    with open(path, "r", encoding="utf-8") as f:
        src = f.read()
    if simple:
        src = src.replace(f"{OLD_COUNT} tests", f"{NEW_COUNT} tests")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(src)
    print(f"  [OK] Updated test count in {path}")

# ROADMAP: update header count and add Phase 18 entry
with open("ROADMAP.md", "r", encoding="utf-8") as f:
    src = f.read()

# Update header
src = src.replace(f"· {OLD_COUNT} tests ·", f"· {NEW_COUNT} tests ·")

# Add Phase 18 entry after Phase 17
phase18_entry = """
### Phase 18 — Observability Dashboard

- [x] **Grafana dashboard**: Pre-built `deploy/grafana/spine-dashboard.json` with 12 panels (sessions, latency, throughput, errors, memory, CPU, prediction, cache, protocol, connections)
- [x] **Prometheus config**: `deploy/prometheus/prometheus.yml` with spine-core + gateway scrape targets
- [x] **Gateway `/metrics` endpoint**: Prometheus-format exposition (uptime, active sessions, requests, errors counters)
- [x] **Gateway request counting**: `AtomicU64` counters for total requests and errors across all API handlers
- [x] **OpenTelemetry tracing**: `#[instrument]` on key agent methods (navigate, get_ur, search, ping, execute_hls) and gateway handlers (navigate, search, execute_hls)
- [x] **Agent tracing dep**: Added `tracing = "0.1"` to spine-agent
- [x] **431 tests passing**: +2 gateway observability tests, 0 failures, 0 Clippy warnings
"""

# Insert after Phase 17's test count line
marker = "- [x] **429 tests passing**: +14 tests"
idx = src.find(marker)
if idx >= 0:
    # Find end of that line
    eol = src.find("\n", idx)
    src = src[:eol+1] + phase18_entry + src[eol+1:]
    print("  [OK] Added Phase 18 entry to ROADMAP")

# Update Planned section - remove Observability Dashboard
src = src.replace(
    """### Observability Dashboard

- [ ] Real-time session monitoring web UI
- [ ] Grafana dashboard templates for Prometheus metrics
- [ ] Distributed tracing visualization (OpenTelemetry)

""",
    "",
)

with open("ROADMAP.md", "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

# copilot-instructions: add Phase 18 entry
ci = ".github/copilot-instructions.md"
with open(ci, "r", encoding="utf-8") as f:
    src = f.read()

phase18_ci = """
### Phase 18: Observability Dashboard

- [x] **Grafana dashboard**: Pre-built JSON with 12 panels for all SPINE metrics
- [x] **Prometheus config**: Scrape targets for spine-core and gateway
- [x] **Gateway `/metrics` endpoint**: Prometheus-format metrics exposition
- [x] **Gateway request counting**: AtomicU64 counters for requests and errors
- [x] **OpenTelemetry tracing**: `#[instrument]` on key agent and gateway methods
- [x] **431 tests passing**: +2 tests, 0 failures, 0 Clippy warnings
"""

# Insert after Phase 17's last line
marker17 = "- [x] **429 tests passing**: +14 tests"
idx = src.find(marker17)
if idx >= 0:
    eol = src.find("\n", idx)
    src = src[:eol+1] + phase18_ci + src[eol+1:]
    print("  [OK] Added Phase 18 entry to copilot-instructions")

with open(ci, "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

# Update gateway test count in README table
with open("README.md", "r", encoding="utf-8") as f:
    src = f.read()

src = src.replace(
    "| spine-gateway   | 5     |",
    "| spine-gateway   | 7     |",
)

with open("README.md", "w", encoding="utf-8", newline="\n") as f:
    f.write(src)
print("  [OK] Updated spine-gateway test count in README table")

print("\nPhase 18 docs updated successfully.")
