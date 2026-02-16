# REST API

The SPINE Gateway exposes a RESTful API with auto-generated OpenAPI documentation.

## Starting the Gateway

```bash
cargo run -p spine-gateway
```

Swagger UI available at `http://localhost:9091/swagger-ui/`

## Endpoints

### Sessions

#### `POST /api/sessions` — Create session

```json
{ "backend_addr": "127.0.0.1:8080" }
```

Response (201):
```json
{ "session_id": "550e8400-e29b-41d4-a716-446655440000", "backend": "127.0.0.1:8080" }
```

#### `DELETE /api/sessions/{id}` — Delete session

Response: 204 No Content

### Browse

#### `POST /api/sessions/{id}/navigate` — Navigate

```json
{ "url": "https://example.com" }
```

#### `GET /api/sessions/{id}/ur` — Get Unified Representation

Response:
```json
{
  "title": "Example Domain",
  "element_count": 5,
  "metadata": { "charset": "utf-8" },
  "raw": { "title": "...", "elements": [...] }
}
```

#### `GET /api/sessions/{id}/html` — Get raw HTML

#### `POST /api/sessions/{id}/search` — Search

```json
{ "query": "rust programming" }
```

#### `GET /api/sessions/{id}/ping` — Ping

Response: `{ "round_trip_ms": 2 }`

### Compute

#### `POST /api/sessions/{id}/execute` — Execute HLS

```json
{ "script": "let x = 42 * 2; x" }
```

Response: `{ "result": 84 }`

#### `POST /api/parse` — Parse HTML (stateless)

```json
{ "html": "<html><head><title>Test</title></head><body>Hello</body></html>" }
```

#### `POST /api/compile` — Compile HLS (stateless)

```json
{ "source": "let x = 1 + 2; x" }
```

Response:
```json
{
  "instruction_count": 5,
  "data_bytes": 0,
  "exported_functions": [],
  "capabilities": []
}
```

### Operations

#### `GET /health`

```json
{ "status": "ok", "uptime_secs": 3600, "active_sessions": 12 }
```

#### `GET /ready`

```json
{ "ready": true, "available_slots": 988 }
```
