# Tiered Caching

The `spine-cache` crate provides a three-level caching system optimized for agentic workloads.

## Architecture

```text
┌─────────────────────────────────────────────────┐
│                 TieredCache                      │
│                                                  │
│  GET:  L1 → miss → L2 → miss → L3 → miss → None│
│  PUT:  Write to all tiers (write-through)        │
│                                                  │
│  ┌──────┐    ┌──────────┐    ┌───────────┐      │
│  │  L1  │    │    L2    │    │    L3     │      │
│  │ LRU  │    │  mmap'd  │    │  Remote   │      │
│  │ RAM  │    │  files   │    │ delegate  │      │
│  └──────┘    └──────────┘    └───────────┘      │
└─────────────────────────────────────────────────┘
```

## Cache Tiers

### L1 — In-Memory LRU

- **Latency**: ~10 ns
- **Capacity**: Configurable max entries and byte size
- **TTL**: Per-entry time-to-live
- **Eviction**: Least-recently-used

### L2 — File-Backed

- **Latency**: ~1 μs
- **Storage**: Memory-mapped files in configurable directory
- **Key Hashing**: SHA-256 for filesystem-safe keys

### L3 — Remote (Trait)

- **Latency**: ~1 ms
- **Interface**: `RemoteCache` trait for pluggable backends (Redis, Memcached, etc.)

## Behavior

- **Read**: Cascading miss from L1 → L2 → L3; on hit, promote to higher tier
- **Write**: Write-through to all populated tiers
- **Invalidation**: Per-key or namespace-wide clear
