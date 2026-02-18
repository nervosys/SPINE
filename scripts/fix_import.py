"""Fix unused IntoResponse import in gateway."""
gw = "spine-gateway/src/main.rs"
with open(gw, "r", encoding="utf-8") as f:
    src = f.read()

src = src.replace(
    "use axum::response::{IntoResponse, Json, Response};",
    "use axum::response::{Json, Response};",
)

with open(gw, "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

print("Fixed IntoResponse import")
