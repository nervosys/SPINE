# Knowledge Base

SPINE provides a distributed CRDT-based knowledge base with four memory subsystems.

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

### Delete

```rust
client.delete_knowledge("rust-homepage").await?;
```

### Propose (Distributed)

In cluster mode, propose knowledge for consensus:

```rust
client.propose_knowledge(
    "shared-finding",
    serde_json::json!({"discovery": "important pattern"}),
    vec!["research".into()],
).await?;
```

## Memory Subsystems

| Subsystem      | Inspiration        | Purpose                                    |
| -------------- | ------------------ | ------------------------------------------ |
| **Episodic**   | Hippocampus        | Event sequences, navigation history        |
| **Semantic**   | Neocortex          | Facts, relationships, structured knowledge |
| **Working**    | Prefrontal cortex  | Active context, current task state         |
| **Collective** | Swarm intelligence | Shared knowledge across agents via CRDTs   |

## CRDT Consistency

Knowledge entries use Conflict-free Replicated Data Types for eventual consistency across distributed nodes. No coordination protocol needed — concurrent writes automatically merge.
