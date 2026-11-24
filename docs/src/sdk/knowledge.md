# Knowledge Base

The daemon keeps a per-session knowledge store that agents write with `store_knowledge` and read with `query_knowledge`. The richer memory subsystems and CRDT replication described at the end of this page are research code, not part of the native commands.

## Basic Operations

### Store

```rust
client.store_knowledge(
    "rust-homepage",
    serde_json::json!({"url": "https://rust-lang.org", "category": "programming"}),
    vec!["language".into(), "systems".into()],
).await?;
```

### Query

```rust
let results = client.query_knowledge(
    "programming languages",
    vec!["language".into()],
    10,  // max results
).await?;
```

The knowledge base is per session on the daemon. There is no delete command in
the core set; entries go when the session expires.

## Memory Subsystems (research)

The following live in `research/spine-knowledge` and are not reachable through the SDK or the wire.

| Subsystem      | Inspiration        | Purpose                                    |
| -------------- | ------------------ | ------------------------------------------ |
| **Episodic**   | Hippocampus        | Event sequences, navigation history        |
| **Semantic**   | Neocortex          | Facts, relationships, structured knowledge |
| **Working**    | Prefrontal cortex  | Active context, current task state         |
| **Collective** | Swarm intelligence | Shared knowledge across agents via CRDTs   |

## CRDT Consistency (research)

In `research/spine-knowledge`, knowledge entries use Conflict-free Replicated Data Types for eventual consistency across distributed nodes. No coordination protocol needed — concurrent writes automatically merge.
