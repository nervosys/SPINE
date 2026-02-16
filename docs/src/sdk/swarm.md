# Swarm Intelligence

## Task Delegation

```rust
// Delegate to any available agent
client.delegate_task("scrape product prices", None).await?;

// Delegate to a specific agent
client.delegate_task("analyze sentiment", Some(agent_id)).await?;
```

## Swarm Search

Distributed search across multiple agents:

```rust
client.swarm_search("quantum computing papers", 3).await?;
```

The `depth` parameter controls how many hops the search propagates.

## Plan Execution

Create and execute multi-step plans:

```rust
// Create a plan
let plan_id = client.create_swarm_plan("Research Rust async patterns").await?;

// Execute individual tasks
client.execute_plan_task(plan_id, task_id).await?;
```

## Session Transfer

Move a session to another cluster node:

```rust
client.transfer_session(target_node_id).await?;
```

## Social Communication

Send speech acts between agents:

```rust
client.send_speech_act(
    target_agent_id,
    "inform",
    "Found 15 relevant results",
).await?;
```

Supported performatives: `inform`, `request`, `propose`, `accept`, `reject`, `query`.

## Autonomous Mode

Enable self-directed exploration:

```rust
client.set_autonomous_mode(true).await?;
```
