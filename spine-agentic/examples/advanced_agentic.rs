//! # Advanced Agentic Web Demonstration
//!
//! This example demonstrates the advanced features of the agentic web stack:
//! - Decentralized Identity (DID)
//! - Protocol Negotiation
//! - Agent Composition
//! - Marketplace
//! - Temporal Reasoning
//! - Context Bridging

use chrono::Utc;
use spine_agentic::{
    agent, AgentCapability, AgentDID, AgentId, AgentMarketplace, AggregationMethod,
    CommunicationProtocol, ComponentRole, CompositeAgent, CompositionStrategy, ContextBridge,
    ContextPermission, ContextPolicy, ListingStatus, MarketplaceListing, MarketplaceQuery,
    NegotiationStatus, PricingModel, ProtocolNegotiation, RetryPolicy, ServiceEndpoint,
    ServiceLevelAgreement, ServiceType, TemporalEvent, TemporalReasoner, TrustLevel,
};
use std::time::Duration;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    println!("╔═══════════════════════════════════════════════════════════════════════╗");
    println!("║                 ADVANCED AGENTIC WEB STACK                            ║");
    println!("║        DID • Composition • Marketplace • Temporal • Context           ║");
    println!("╚═══════════════════════════════════════════════════════════════════════╝\n");

    demo_decentralized_identity().await;
    demo_protocol_negotiation().await;
    demo_agent_composition().await;
    demo_marketplace().await;
    demo_temporal_reasoning().await;
    demo_context_bridging().await;
    demo_agent_builder().await;

    println!("\n═══════════════════════════════════════════════════════════════════════");
    println!("                 ADVANCED AGENTIC WEB DEMO COMPLETE!");
    println!("═══════════════════════════════════════════════════════════════════════");
}

async fn demo_decentralized_identity() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 1. DECENTRALIZED IDENTITY (DID)                                       │");
    println!("└───────────────────────────────────────────────────────────────────────┘\n");

    // Generate agent identity
    let mut did = AgentDID::generate("ResearchAgent");

    println!("Generated Agent DID:");
    println!("  Method:     {}", did.method);
    println!("  Identifier: {}", did.identifier);
    println!("  Full DID:   {}", did.to_string());
    println!("  Created:    {}", did.created);

    // Add service endpoints
    did.add_service(ServiceEndpoint {
        id: format!("{}#messaging", did.to_string()),
        service_type: ServiceType::AgentMessaging,
        endpoint: "wss://agent.example.com/msg".to_string(),
        protocols: vec!["chameleon-v1".to_string(), "semantic-json-v1".to_string()],
    });

    did.add_service(ServiceEndpoint {
        id: format!("{}#knowledge", did.to_string()),
        service_type: ServiceType::KnowledgeQuery,
        endpoint: "https://agent.example.com/knowledge".to_string(),
        protocols: vec!["sparql".to_string()],
    });

    println!("\n  Service Endpoints:");
    for svc in &did.document.service {
        println!("    - {:?}: {}", svc.service_type, svc.endpoint);
    }

    // Sign and verify a message
    let message = b"Hello, Agent World!";
    let signature = did.sign(message);
    let verified = did.verify(message, &signature);

    println!("\n  Signature Test:");
    println!("    Message:  \"Hello, Agent World!\"");
    println!("    Verified: {}", verified);
    println!();
}

async fn demo_protocol_negotiation() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 2. SEMANTIC PROTOCOL NEGOTIATION                                      │");
    println!("└───────────────────────────────────────────────────────────────────────┘\n");

    // Agent A proposes protocols
    let agent_a = "did:agent:alice123";
    let agent_b = "did:agent:bob456";

    let mut negotiation = ProtocolNegotiation::initiate(
        agent_a,
        agent_b,
        vec![
            CommunicationProtocol::LatentSpace {
                encoder: "titans-v2".to_string(),
                dimension: 256,
            },
            CommunicationProtocol::SemanticJSON {
                schema_version: "2.0".to_string(),
            },
            CommunicationProtocol::NaturalLanguage {
                language: "en".to_string(),
                embedding_model: "text-embedding-3".to_string(),
            },
        ],
    );

    println!("Protocol Negotiation:");
    println!("  Initiator: {}", agent_a);
    println!("  Responder: {}", agent_b);
    println!("\n  Proposed Protocols:");
    for (i, proto) in negotiation.proposed_protocols.iter().enumerate() {
        println!("    {}. {:?}", i + 1, proto);
    }

    // Agent B responds with acceptable protocols
    let agent_b_acceptable = vec![
        CommunicationProtocol::SemanticJSON {
            schema_version: "2.0".to_string(),
        },
        CommunicationProtocol::BinaryCompact {
            compression: "zstd".to_string(),
        },
    ];

    println!("\n  Responder's Acceptable Protocols:");
    for proto in &agent_b_acceptable {
        println!("    - {:?}", proto);
    }

    let _agreed = negotiation.respond(&agent_b_acceptable);

    match negotiation.status {
        NegotiationStatus::Agreed => {
            println!("\n  ✓ NEGOTIATION SUCCEEDED");
            if let Some(proto) = &negotiation.selected {
                println!("    Agreed Protocol: {:?}", proto);
            }
        }
        NegotiationStatus::Failed { ref reason } => {
            println!("\n  ✗ NEGOTIATION FAILED: {}", reason);
        }
        _ => {}
    }
    println!();
}

async fn demo_agent_composition() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 3. EMERGENT AGENT COMPOSITION                                         │");
    println!("└───────────────────────────────────────────────────────────────────────┘\n");

    // Create a composite agent from multiple specialists
    let mut composite = CompositeAgent::new(
        "ResearchTeam",
        CompositionStrategy::Parallel {
            aggregation: AggregationMethod::BestConfidence,
        },
    );

    // Add component agents
    let data_miner = AgentId::new();
    let analyst = AgentId::new();
    let writer = AgentId::new();
    let validator = AgentId::new();

    composite.add_component(
        data_miner,
        ComponentRole::Specialist {
            capability: AgentCapability::ContentExtraction,
        },
        1.0,
    );

    composite.add_component(
        analyst,
        ComponentRole::Specialist {
            capability: AgentCapability::AutonomousDecision,
        },
        0.9,
    );

    composite.add_component(
        writer,
        ComponentRole::Primary {
            domains: vec!["writing".to_string(), "synthesis".to_string()],
        },
        0.8,
    );

    composite.add_component(validator, ComponentRole::Validator, 1.0);

    println!("Composite Agent: {}", composite.name);
    println!("  ID: {}", composite.composite_id.0);
    println!("  Strategy: {:?}", composite.strategy);
    println!("\n  Components:");
    for (i, comp) in composite.components.iter().enumerate() {
        println!(
            "    {}. {} ({:?}, weight: {:.1})",
            i + 1,
            &comp.agent_id.0.to_string()[..8],
            comp.role,
            comp.weight
        );
    }

    // Test routing
    let extraction_routes = composite.route(&AgentCapability::ContentExtraction);
    let reasoning_routes = composite.route(&AgentCapability::AutonomousDecision);

    println!("\n  Routing Test:");
    println!(
        "    ContentExtraction → {} agent(s)",
        extraction_routes.len()
    );
    println!("    Reasoning → {} agent(s)", reasoning_routes.len());
    println!();
}

async fn demo_marketplace() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 4. AGENT MARKETPLACE                                                  │");
    println!("└───────────────────────────────────────────────────────────────────────┘\n");

    let marketplace = AgentMarketplace::new();

    // List some services
    let provider1 = AgentId::new();
    let provider2 = AgentId::new();
    let provider3 = AgentId::new();

    let listing1 = MarketplaceListing {
        id: Uuid::new_v4(),
        provider: provider1,
        title: "Premium Research Assistant".to_string(),
        description: "High-accuracy research with academic sources".to_string(),
        capabilities: vec![
            AgentCapability::ContentExtraction,
            AgentCapability::AutonomousDecision,
        ],
        pricing: PricingModel::PerRequest { credits: 50 },
        sla: ServiceLevelAgreement {
            max_response_time_ms: 3000,
            uptime_guarantee: 0.995,
            accuracy_guarantee: Some(0.95),
            retry_policy: RetryPolicy::default(),
        },
        examples: vec![],
        status: ListingStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let listing2 = MarketplaceListing {
        id: Uuid::new_v4(),
        provider: provider2,
        title: "Free Translation Bot".to_string(),
        description: "Basic translation service".to_string(),
        capabilities: vec![AgentCapability::Custom("translation".to_string())],
        pricing: PricingModel::Free,
        sla: ServiceLevelAgreement {
            max_response_time_ms: 10000,
            uptime_guarantee: 0.9,
            accuracy_guarantee: None,
            retry_policy: RetryPolicy::default(),
        },
        examples: vec![],
        status: ListingStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let listing3 = MarketplaceListing {
        id: Uuid::new_v4(),
        provider: provider3,
        title: "Enterprise Data Analysis".to_string(),
        description: "Premium data analysis with custom models".to_string(),
        capabilities: vec![
            AgentCapability::KnowledgeManagement,
            AgentCapability::AutonomousDecision,
        ],
        pricing: PricingModel::Subscription {
            credits_per_period: 500,
            period_days: 30,
        },
        sla: ServiceLevelAgreement {
            max_response_time_ms: 1000,
            uptime_guarantee: 0.999,
            accuracy_guarantee: Some(0.99),
            retry_policy: RetryPolicy::default(),
        },
        examples: vec![],
        status: ListingStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    marketplace.list_service(listing1);
    marketplace.list_service(listing2);
    marketplace.list_service(listing3);

    println!("Marketplace Listings: 3 services registered");

    // Search marketplace
    let query = MarketplaceQuery {
        capability: Some(AgentCapability::AutonomousDecision),
        max_credits: Some(100),
        min_reputation: None,
        limit: Some(10),
        text: None,
    };

    let results = marketplace.search(&query);

    println!("\n  Search: Reasoning capability, max 100 credits");
    println!("  Results: {} listing(s)", results.len());
    for listing in &results {
        println!("    - {} ({:?})", listing.title, listing.pricing);
    }

    // Procure a service
    let consumer = AgentId::new();
    if let Some(first) = results.first() {
        match marketplace.procure(first.id, consumer).await {
            Ok(tx_id) => {
                println!("\n  ✓ Transaction initiated: {}", &tx_id.to_string()[..8]);

                // Complete transaction
                marketplace
                    .complete_transaction(
                        tx_id,
                        true,
                        Some(5),
                        Some("Excellent service!".to_string()),
                    )
                    .await;

                if let Some(rep) = marketplace.get_reputation(&first.provider) {
                    println!("  Provider reputation updated:");
                    println!("    Score: {:.1}", rep.score);
                    println!("    Completed: {}", rep.completed);
                    println!("    Avg Rating: {:.1}", rep.avg_rating);
                }
            }
            Err(e) => println!("  ✗ Transaction failed: {}", e),
        }
    }
    println!();
}

async fn demo_temporal_reasoning() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 5. TEMPORAL REASONING ENGINE                                          │");
    println!("└───────────────────────────────────────────────────────────────────────┘\n");

    let reasoner = TemporalReasoner::new();

    // Record a sequence of causally related events
    let event1_id = Uuid::new_v4();
    let event2_id = Uuid::new_v4();
    let event3_id = Uuid::new_v4();

    let event1 = TemporalEvent {
        id: event1_id,
        event_type: "user_request".to_string(),
        timestamp: Utc::now() - chrono::Duration::minutes(5),
        duration: None,
        data: serde_json::json!({ "query": "Find AI research papers" }),
        causes: vec![],
        effects: vec![event2_id],
        confidence: 1.0,
    };

    let event2 = TemporalEvent {
        id: event2_id,
        event_type: "search_initiated".to_string(),
        timestamp: Utc::now() - chrono::Duration::minutes(4),
        duration: Some(Duration::from_secs(30)),
        data: serde_json::json!({ "sources": ["arxiv", "semantic-scholar"] }),
        causes: vec![event1_id],
        effects: vec![event3_id],
        confidence: 1.0,
    };

    let event3 = TemporalEvent {
        id: event3_id,
        event_type: "results_delivered".to_string(),
        timestamp: Utc::now() - chrono::Duration::minutes(3),
        duration: None,
        data: serde_json::json!({ "papers_found": 42 }),
        causes: vec![event2_id],
        effects: vec![],
        confidence: 1.0,
    };

    reasoner.record_event(event1.clone()).await;
    reasoner.record_event(event2.clone()).await;
    reasoner.record_event(event3.clone()).await;

    println!("Temporal Timeline:");
    println!(
        "  Event 1: {} at {}",
        event1.event_type,
        event1.timestamp.format("%H:%M:%S")
    );
    println!("      ↓ (causes)");
    println!(
        "  Event 2: {} at {}",
        event2.event_type,
        event2.timestamp.format("%H:%M:%S")
    );
    println!("      ↓ (causes)");
    println!(
        "  Event 3: {} at {}",
        event3.event_type,
        event3.timestamp.format("%H:%M:%S")
    );

    // Find causal chain
    if let Some(chain) = reasoner.find_causal_chain(event1_id, event3_id).await {
        println!("\n  Causal Chain: {} events", chain.len());
        for (i, id) in chain.iter().enumerate() {
            println!("    {}. {}", i + 1, &id.to_string()[..8]);
        }
    }

    // Make a prediction
    let prediction_id = reasoner.predict(
        "user_followup",
        vec![
            "User typically follows up after receiving results".to_string(),
            "Pattern observed in 85% of similar sessions".to_string(),
        ],
        vec![event3_id],
    );

    println!("\n  Prediction: {}", &prediction_id.to_string()[..8]);
    println!("    Type: user_followup");
    println!("    Based on: {} event(s)", 1);
    println!();
}

async fn demo_context_bridging() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 6. CONTEXT BRIDGING                                                   │");
    println!("└───────────────────────────────────────────────────────────────────────┘\n");

    let mut bridge = ContextBridge::new();

    // Create agents
    let agent_a = AgentId::new();
    let agent_b = AgentId::new();
    let agent_c = AgentId::new();

    // Create a shared context pool
    let pool_name = bridge.create_pool(
        "research-session",
        agent_a,
        serde_json::json!({
            "topic": "quantum computing",
            "papers_found": 0,
            "key_concepts": []
        }),
    );

    println!("Context Pool Created: {}", pool_name);
    println!("  Owner: {}", &agent_a.0.to_string()[..8]);

    // Agent B joins
    bridge.join_pool(&pool_name, agent_b).unwrap();
    println!("  Agent {} joined", &agent_b.0.to_string()[..8]);

    // Add access policy
    bridge.add_policy(ContextPolicy {
        pool_pattern: "research-*".to_string(),
        allowed_agents: vec![agent_c],
        allowed_capabilities: vec![AgentCapability::ContentExtraction],
        permission: ContextPermission::Read,
        expiry: None,
    });

    println!(
        "  Policy added: read access for agent {}",
        &agent_c.0.to_string()[..8]
    );

    // Share context updates
    bridge
        .share(
            &pool_name,
            serde_json::json!({
                "papers_found": 15,
                "key_concepts": ["qubits", "entanglement", "superposition"]
            }),
            &agent_a,
        )
        .unwrap();

    println!("\n  Context updated by owner:");

    // Read context
    let ctx = bridge.read(&pool_name, &agent_b).unwrap();
    println!("  Agent B reads context:");
    println!("    Topic: {}", ctx["topic"]);
    println!("    Papers: {}", ctx["papers_found"]);
    println!("    Concepts: {:?}", ctx["key_concepts"]);

    // List accessible pools
    let pools_a = bridge.list_accessible_pools(&agent_a);
    let pools_b = bridge.list_accessible_pools(&agent_b);

    println!("\n  Accessible pools:");
    println!("    Agent A: {:?}", pools_a);
    println!("    Agent B: {:?}", pools_b);
    println!();
}

async fn demo_agent_builder() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 7. FLUENT AGENT BUILDER                                               │");
    println!("└───────────────────────────────────────────────────────────────────────┘\n");

    // Build an agent with the fluent API
    let system = agent("MarketAnalyst")
        .with_capabilities(vec![
            AgentCapability::ContentExtraction,
            AgentCapability::KnowledgeManagement,
            AgentCapability::AutonomousDecision,
        ])
        .with_trust(TrustLevel::Verified)
        .with_did()
        .with_marketplace(
            "Market Analysis Service",
            "Real-time market analysis with AI insights",
            PricingModel::PerRequest { credits: 25 },
        )
        .with_protocols(vec![
            CommunicationProtocol::LatentSpace {
                encoder: "titans-v2".to_string(),
                dimension: 256,
            },
            CommunicationProtocol::SemanticJSON {
                schema_version: "1.0".to_string(),
            },
        ])
        .build()
        .await;

    println!("Agent Built: {}", system.runtime.profile().name);
    println!("  ID: {}", system.runtime.profile().id.0);
    println!("  Trust: {:?}", system.runtime.profile().trust_level);
    println!(
        "  Capabilities: {}",
        system.runtime.profile().capabilities.len()
    );

    println!("\n  Architecture:");
    println!("    ┌─────────────────────────────────────┐");
    println!("    │           AgentSystem               │");
    println!("    ├─────────────────────────────────────┤");
    println!("    │  ┌─────────┐  ┌─────────────────┐  │");
    println!("    │  │ Runtime │──│ExecutionEngine  │  │");
    println!("    │  └─────────┘  └─────────────────┘  │");
    println!("    │       │              │             │");
    println!("    │  ┌────┴────┐  ┌──────┴──────┐     │");
    println!("    │  │ Swarm   │  │  Behavior   │     │");
    println!("    │  │Coordin. │  │  Executor   │     │");
    println!("    │  └─────────┘  └─────────────┘     │");
    println!("    │       │                           │");
    println!("    │  ┌────┴────┐                      │");
    println!("    │  │Registry │                      │");
    println!("    │  └─────────┘                      │");
    println!("    └─────────────────────────────────────┘");
    println!();
}
