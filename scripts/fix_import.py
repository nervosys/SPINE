# HISTORICAL one-shot patch script (checked 2026-11-21). Its edits were applied long ago;
# re-running it can overwrite later changes to the files it targets. Not part of any build or CI job.
"""Fix unused IntoResponse import in gateway."""
gw = "src/spine-gateway/src/main.rs"
with open(gw, "r", encoding="utf-8") as f:
    src = f.read()

src = src.replace(
    "use axum::response::{IntoResponse, Json, Response};",
    "use axum::response::{Json, Response};",
)

with open(gw, "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

print("Fixed IntoResponse import")
