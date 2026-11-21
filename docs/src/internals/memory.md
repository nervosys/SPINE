# Memory System

`spine-knowledge` provides a unified bioinspired memory architecture.

**Research.** `spine-knowledge` and the MIRAS variants in `spine-crypto` live in the separate `research/` workspace. The daemon does not use them: the native `store_knowledge` and `query_knowledge` commands work on a per-session key/value store (see [Knowledge Base](../sdk/knowledge.md)).

## Subsystems

### Episodic Memory (Hippocampus)

Stores sequences of events with temporal ordering:
- Navigation history
- Interaction sequences
- Temporal context for retrieval

### Semantic Memory (Neocortex)

Structured knowledge storage:
- Facts and relationships
- Tagged entries for filtered retrieval
- CRDT-based distributed consistency

### Working Memory (Prefrontal Cortex)

Active task context:
- Current page state
- Recent computations
- Task-relevant information

### Collective Memory (Swarm Intelligence)

Shared knowledge across agents:
- CRDT merge for conflict-free replication
- Consensus-based proposals for important knowledge
- Cross-agent discovery and search

## MIRAS Variants

Three memory architectures from `spine-crypto`:

| Variant | Full Name                                  | Focus                              |
| ------- | ------------------------------------------ | ---------------------------------- |
| YAAD    | Yet Another Anomaly Detector               | Novelty detection, anomaly scoring |
| MONETA  | Memory-Oriented Neural Encoding            | Persistent memory traces           |
| MEMORA  | Memory-Enhanced Model for Organized Recall | Structured recall patterns         |

## Distributed Consistency

All knowledge entries use CRDTs (Conflict-free Replicated Data Types):
- **Last-Writer-Wins Register** for values
- **Grow-Only Set** for tags
- **Version vectors** for causal ordering
- No coordination protocol needed — concurrent writes merge automatically
