# spine-go

Go bindings for the [SPINE](https://github.com/nervosys/SPINE) agentic web stack.

## Prerequisites

1. **Rust toolchain** — Install from <https://rustup.rs>
2. **Go 1.21+** — Install from <https://go.dev/dl>

## Building the FFI Library

```bash
# From the SPINE repository root:
cd spine-ffi
cargo build --release
```

This produces `target/release/libspine_ffi.so` (Linux), `libspine_ffi.dylib` (macOS), or `spine_ffi.dll` (Windows).

## Usage

```go
package main

import (
    "fmt"
    "log"

    spine "github.com/nervosys/spine-go"
)

func main() {
    fmt.Println("SPINE version:", spine.Version())

    // Offline HTML parsing (no server needed)
    ur, err := spine.ParseHTML("<html><head><title>Hello</title></head><body>World</body></html>")
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Title: %s, Elements: %d\n", ur.Title, len(ur.Elements))

    // Offline HLS compilation
    binary, err := spine.CompileHLS("let x = 42 * 2; x")
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Instructions: %d\n", len(binary.Instructions))

    // Connect to a SPINE server
    client, err := spine.Connect("127.0.0.1:8080")
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Navigate
    if err := client.Navigate("https://example.com"); err != nil {
        log.Fatal(err)
    }

    // Get Unified Representation
    page, err := client.GetUR()
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Page title: %s\n", page.Title)

    // Ping
    rtt, err := client.Ping()
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("RTT: %d ms\n", rtt)
}
```

## Building & Testing

```bash
# Set library paths (adjust for your OS)
export CGO_LDFLAGS="-L../../target/release -lspine_ffi"
export CGO_CFLAGS="-I../spine-ffi/include"
export LD_LIBRARY_PATH="../../target/release:$LD_LIBRARY_PATH"

# Build
go build ./...

# Test
go test -v ./...
```

## API Reference

| Function                                    | Description                        |
| ------------------------------------------- | ---------------------------------- |
| `Connect(addr)`                             | Connect to a SPINE server over TCP |
| `Client.Navigate(url)`                      | Navigate to a URL                  |
| `Client.GetUR()`                            | Get Unified Representation         |
| `Client.GetRawHTML()`                       | Get raw HTML                       |
| `Client.Search(query)`                      | Search the web                     |
| `Client.ExecuteHLS(script)`                 | Execute HLS script                 |
| `Client.Ping()`                             | Ping server (returns RTT ms)       |
| `Client.Morph()`                            | Trigger protocol morphing          |
| `Client.GetCapabilities()`                  | Get server capabilities            |
| `Client.StoreKnowledge(key, value, tags)`   | Store knowledge entry              |
| `Client.QueryKnowledge(query, tags, limit)` | Query knowledge base               |
| `Client.Close()`                            | Disconnect                         |
| `ParseHTML(html)`                           | Parse HTML offline                 |
| `CompileHLS(source)`                        | Compile HLS offline                |
| `Version()`                                 | Get library version                |

## License

Apache-2.0 — Copyright (c) 2024-2026 Nervosys LLC
