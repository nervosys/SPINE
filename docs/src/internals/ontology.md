# Ontology & Agent Discovery

SPINE includes a rich ontology system for agent self-description and mutual discovery.

## Overview

Every agent publishes an **AgentOntology** — a namespace-versioned vocabulary of capabilities, roles, and domain expertise. The ontology system enables:

- **Self-description**: Agents declare what they can do, what data they handle, and what protocols they speak.
- **Discovery**: The `OntologyRegistry` indexes terms for lookup by URI, hash, or neural similarity.
- **Privacy**: Per-term visibility controls (Public, HashOnly, NeuralHash, Private) let agents disclose selectively.
- **Compatibility scoring**: Jaccard similarity between agents' public terms identifies good collaborators.

## Core Types

| Type | Purpose |
|------|---------|
| `OntologyTerm` | URI-based term with label, description, parent hierarchy, properties |
| `AgentOntology` | Namespace-versioned ontology with SHA-256 whole-ontology hash |
| `DisclosedOntology` | Privacy-preserving view combining cleartext, hashed, and neural terms |
| `OntologyAccessControl` | Per-agent permission rules with first-match-wins resolution |
| `OntologyRegistry` | Discovery index with term lookup, hash verification, neural similarity |

## Pre-Built Vocabularies

The `ontology_vocab` module provides 230+ hierarchical terms across 16 categories:

- **Capabilities**: 9 top-level domains with 150+ leaf terms (web, data, security, NLP, etc.)
- **Roles**: 15 agent roles (researcher, sentinel, coordinator, etc.)
- **Domains**: 15 application domains (finance, healthcare, IoT, etc.)
- **Protocol/IO/QoS/Security/Hardware**: 65 descriptors for interop and runtime constraints

Six role-specific pre-built ontologies are available via `build_*_ontology()` constructors:
`web_researcher`, `security_sentinel`, `iot_edge`, `data_pipeline`, `swarm_coordinator`, `ml_inference`.

## Visibility Controls

```text
Public      → Term visible in cleartext to all agents
HashOnly    → Only SHA-256 hash disclosed; verifiable but not browsable
NeuralHash  → Locality-sensitive embedding for approximate matching
Private     → Never disclosed
```

## Usage

```rust,ignore
use spine_agentic::ontology_vocab::build_web_researcher_ontology;

let ontology = build_web_researcher_ontology();
let disclosed = ontology.disclose_public();
// Register with the discovery index
registry.register_ontology(&agent_id, &disclosed);
```