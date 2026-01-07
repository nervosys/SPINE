//! # Agentic Web Demo
//!
//! Demonstrates the revolutionary agentic web stack where AI agents are first-class
//! citizens of the web.
//!
//! Run: cargo run --example agentic_web_demo -p hyperlight-agentic

use spine_agentic::{
    create_agent, create_agent_with_capabilities,
    AgentCapability, Goal, ResourceLocator, SemanticQuery, OutputType,
    IntentionStatus, MessageContent, TrustLevel, SwarmTask, SwarmRole,
    Action, Condition, Plan, KnowledgeNode, KnowledgeGraph,
};
use chrono::Utc;
use uuid::Uuid;

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════════════╗");
    println!("║             SPINE Agentic WEB STACK DEMONSTRATION                ║");
    println!("║                   The Future of Agent-Native Web                      ║");
    println!("╚═══════════════════════════════════════════════════════════════════════╝");
    println!();

    demo_agent_creation();
    demo_intentions_and_goals();
    demo_resource_locators();
    demo_semantic_queries();
    demo_planning();
    demo_knowledge_graph();
    demo_swarm_formation();
    demo_agent_communication();
    
    println!();
    println!("═══════════════════════════════════════════════════════════════════════");
    println!("                    AGENTIC WEB STACK DEMO COMPLETE!");
    println!("═══════════════════════════════════════════════════════════════════════");
    println!();
    println!("The agentic web is not an evolution of the human web—");
    println!("it's a parallel universe where AI agents are native inhabitants.");
    println!();
}

fn demo_agent_creation() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 1. AGENT IDENTITY & CAPABILITIES                                      │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    // Create a simple agent
    let agent = create_agent("Navigator-Alpha");
    println!("✓ Created agent: {}", agent.profile().name);
    println!("  ID: {:?}", agent.agent_id());
    println!("  Trust Level: {:?}", agent.profile().trust_level);
    println!("  MIRAS Variant: {}", agent.profile().miras_variant);
    
    // Create a specialized agent
    let specialist = create_agent_with_capabilities(
        "Knowledge-Curator",
        vec![
            AgentCapability::ContentExtraction,
            AgentCapability::KnowledgeManagement,
            AgentCapability::ContinualLearning,
            AgentCapability::SwarmParticipation,
        ],
    );
    
    println!();
    println!("✓ Created specialist agent: {}", specialist.profile().name);
    println!("  Capabilities:");
    for cap in &specialist.profile().capabilities {
        println!("    • {:?}", cap);
    }
    println!();
}

fn demo_intentions_and_goals() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 2. INTENTIONS & GOALS                                                 │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();
    
    let agent = create_agent("Research-Agent");
    
    // Navigation goal
    let nav_intention = agent.intend(Goal::Navigate {
        target: ResourceLocator::url("https://arxiv.org/abs/2401.00001"),
    });
    println!("✓ Created navigation intention: {:?}", nav_intention);
    
    // Extraction goal with semantic query
    let extract_intention = agent.intend(Goal::Extract {
        query: SemanticQuery {
            query: "What are the main contributions of this paper?".to_string(),
            output_type: OutputType::List,
            context: vec!["academic paper".to_string(), "AI research".to_string()],
            confidence_threshold: 0.8,
        },
        from: ResourceLocator::semantic("recent AI papers")
            .with_constraint("topic:transformers")
            .with_constraint("year:2024"),
    });
    println!("✓ Created extraction intention: {:?}", extract_intention);
    
    // Learning goal
    let learn_intention = agent.intend(Goal::Learn {
        topic: "Mixture of Experts architectures".to_string(),
        depth: spine_agentic::LearningDepth::Deep,
    });
    println!("✓ Created learning intention: {:?}", learn_intention);
    
    // Custom goal
    let custom_intention = agent.intend(Goal::Custom {
        description: "Synthesize findings across multiple papers".to_string(),
        parameters: serde_json::json!({
            "papers": ["paper1", "paper2", "paper3"],
            "focus": "performance comparisons",
            "output_format": "structured_table"
        }),
    });
    println!("✓ Created custom intention: {:?}", custom_intention);
    println!();
}

fn demo_resource_locators() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 3. SEMANTIC RESOURCE LOCATORS                                         │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();
    
    // Traditional URL
    let url_loc = ResourceLocator::url("https://example.com/data.json");
    println!("URL Locator: {:?}", url_loc);
    
    // Semantic locator - find by concept, not address
    let semantic_loc = ResourceLocator::semantic("weather forecast")
        .with_constraint("location:san-francisco")
        .with_constraint("timeframe:next-week")
        .with_constraint("format:hourly");
    println!();
    println!("Semantic Locator: {:?}", semantic_loc);
    println!("  → Agents navigate by MEANING, not by URLs");
    
    // Agent-relative path
    let agent_id = spine_agentic::AgentId::new();
    let agent_path = ResourceLocator::AgentPath {
        agent: agent_id,
        path: "/knowledge/recent-discoveries".to_string(),
    };
    println!();
    println!("Agent Path: {:?}", agent_path);
    println!("  → Resources exist in agent namespaces");
    
    // Latent space coordinates - navigate neural space
    let latent_loc = ResourceLocator::LatentCoord {
        space: "academic-knowledge".to_string(),
        coordinates: vec![0.5, -0.3, 0.8, 0.1],
    };
    println!();
    println!("Latent Coordinates: {:?}", latent_loc);
    println!("  → Navigate semantic embedding space directly");
    
    // Content-addressed
    let content_loc = ResourceLocator::ContentAddress(
        "sha256:a1b2c3d4e5f6...".to_string()
    );
    println!();
    println!("Content Address: {:?}", content_loc);
    println!("  → Immutable references to knowledge");
    println!();
}

fn demo_semantic_queries() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 4. SEMANTIC QUERIES                                                   │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();
    
    // Natural language query
    let query1 = SemanticQuery {
        query: "Find all mentions of gradient descent optimization".to_string(),
        output_type: OutputType::List,
        context: vec![
            "machine learning".to_string(),
            "technical documentation".to_string(),
        ],
        confidence_threshold: 0.85,
    };
    println!("Query 1 (List): {}", query1.query);
    println!("  Output: {:?}, Confidence: {}+", query1.output_type, query1.confidence_threshold);
    
    // Structured extraction
    let query2 = SemanticQuery {
        query: "Extract company financial metrics".to_string(),
        output_type: OutputType::Json(Some(serde_json::json!({
            "revenue": "number",
            "profit": "number",
            "growth_rate": "percentage"
        }))),
        context: vec!["10-K filing".to_string()],
        confidence_threshold: 0.9,
    };
    println!();
    println!("Query 2 (Structured JSON): {}", query2.query);
    
    // Latent vector output - for agent-to-agent communication
    let query3 = SemanticQuery {
        query: "Encode the key insights from this document".to_string(),
        output_type: OutputType::LatentVector,
        context: vec![],
        confidence_threshold: 0.7,
    };
    println!();
    println!("Query 3 (Latent Vector): {}", query3.query);
    println!("  → Outputs can be neural embeddings for agent consumption");
    println!();
}

fn demo_planning() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 5. PLANNING & REASONING                                               │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();
    
    let mut plan = Plan::new();
    
    // Build a multi-step plan
    let step1 = plan.add_step(spine_agentic::PlanStep {
        id: Uuid::new_v4(),
        action: Action::Navigate(ResourceLocator::semantic("research database")),
        preconditions: vec![],
        postconditions: vec![Condition::ResourceExists(
            ResourceLocator::semantic("research database")
        )],
        estimated_duration: std::time::Duration::from_secs(5),
        retry_policy: Default::default(),
    });
    
    let step2 = plan.add_step(spine_agentic::PlanStep {
        id: Uuid::new_v4(),
        action: Action::Extract(SemanticQuery {
            query: "Find relevant papers".to_string(),
            output_type: OutputType::List,
            context: vec![],
            confidence_threshold: 0.8,
        }),
        preconditions: vec![Condition::ResourceExists(
            ResourceLocator::semantic("research database")
        )],
        postconditions: vec![],
        estimated_duration: std::time::Duration::from_secs(30),
        retry_policy: Default::default(),
    });
    
    let step3 = plan.add_step(spine_agentic::PlanStep {
        id: Uuid::new_v4(),
        action: Action::Parallel(vec![
            Action::Learn { topic: "Topic A".to_string() },
            Action::Learn { topic: "Topic B".to_string() },
            Action::Learn { topic: "Topic C".to_string() },
        ]),
        preconditions: vec![],
        postconditions: vec![],
        estimated_duration: std::time::Duration::from_secs(60),
        retry_policy: Default::default(),
    });
    
    // Add dependencies
    plan.add_dependency(step1, step2);
    plan.add_dependency(step2, step3);
    
    println!("✓ Created plan with {} steps", plan.steps.len());
    println!("  Dependencies: {:?}", plan.dependencies);
    println!();
    println!("Plan structure:");
    println!("  Step 1: Navigate to research database");
    println!("       ↓");
    println!("  Step 2: Extract relevant papers");
    println!("       ↓");
    println!("  Step 3: Learn 3 topics IN PARALLEL");
    println!();
    
    // Conditional action
    let conditional = Action::Branch {
        condition: Condition::ValueEquals {
            path: "result.count".to_string(),
            expected: serde_json::json!(0),
        },
        if_true: Box::new(Action::Custom {
            name: "expand_search".to_string(),
            params: serde_json::json!({}),
        }),
        if_false: Box::new(Action::Custom {
            name: "process_results".to_string(),
            params: serde_json::json!({}),
        }),
    };
    println!("✓ Conditional actions supported");
    println!("  if results.count == 0 → expand_search");
    println!("  else → process_results");
    println!();
}

fn demo_knowledge_graph() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 6. KNOWLEDGE GRAPH                                                    │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();
    
    let mut kg = KnowledgeGraph::new();
    
    // Add nodes with embeddings
    kg.add_node(KnowledgeNode {
        id: "transformer".to_string(),
        label: "Transformer Architecture".to_string(),
        node_type: "concept".to_string(),
        properties: serde_json::json!({
            "introduced": 2017,
            "paper": "Attention Is All You Need"
        }),
        embedding: Some(vec![0.8, 0.2, 0.5, 0.1]),
        confidence: 0.95,
        source: Some(ResourceLocator::url("https://arxiv.org/abs/1706.03762")),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    
    kg.add_node(KnowledgeNode {
        id: "attention".to_string(),
        label: "Self-Attention Mechanism".to_string(),
        node_type: "concept".to_string(),
        properties: serde_json::json!({
            "type": "mechanism",
            "complexity": "O(n²)"
        }),
        embedding: Some(vec![0.7, 0.3, 0.6, 0.2]),
        confidence: 0.98,
        source: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    
    kg.add_node(KnowledgeNode {
        id: "bert".to_string(),
        label: "BERT".to_string(),
        node_type: "model".to_string(),
        properties: serde_json::json!({
            "type": "encoder-only",
            "params": "340M"
        }),
        embedding: Some(vec![0.75, 0.25, 0.55, 0.15]),
        confidence: 0.99,
        source: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    
    // Add relationships
    kg.add_edge("bert", "transformer", spine_agentic::KnowledgeEdge {
        relation: "based_on".to_string(),
        weight: 1.0,
        properties: serde_json::json!({}),
        source: None,
    });
    
    kg.add_edge("transformer", "attention", spine_agentic::KnowledgeEdge {
        relation: "uses".to_string(),
        weight: 1.0,
        properties: serde_json::json!({ "key_component": true }),
        source: None,
    });
    
    println!("✓ Built knowledge graph with 3 nodes and 2 edges");
    println!();
    
    // Query by semantic similarity
    let query_embedding = vec![0.78, 0.22, 0.52, 0.12];
    let similar = kg.query_similar(&query_embedding, 3);
    
    println!("Semantic similarity query:");
    println!("  Query vector: {:?}", query_embedding);
    println!("  Results:");
    for (id, similarity) in similar {
        println!("    • {} (similarity: {:.4})", id, similarity);
    }
    println!();
}

fn demo_swarm_formation() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 7. COLLECTIVE INTELLIGENCE (SWARMS)                                   │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();
    
    // Define a complex task that requires a swarm
    let task = SwarmTask {
        id: Uuid::new_v4(),
        description: "Comprehensive market analysis across 50 sectors".to_string(),
        goal: Box::new(Goal::Custom {
            description: "Analyze market trends and produce consolidated report".to_string(),
            parameters: serde_json::json!({
                "sectors": 50,
                "depth": "comprehensive",
                "output": "executive_summary"
            }),
        }),
        min_members: 5,
        max_members: 20,
        required_capabilities: vec![
            AgentCapability::ContentExtraction,
            AgentCapability::KnowledgeManagement,
            AgentCapability::ContinualLearning,
        ],
        deadline: Some(Utc::now() + chrono::Duration::hours(24)),
    };
    
    println!("✓ Defined swarm task: {}", task.description);
    println!("  Min members: {}", task.min_members);
    println!("  Max members: {}", task.max_members);
    println!("  Required capabilities: {:?}", task.required_capabilities.len());
    println!();
    
    // Swarm roles
    println!("Swarm role distribution:");
    println!("  {:?} - Leads and coordinates", SwarmRole::Leader);
    println!("  {:?} - Distributes sub-tasks", SwarmRole::Coordinator);
    println!("  {:?} - Executes assigned tasks", SwarmRole::Worker);
    println!("  {:?} - Validates results", SwarmRole::Validator);
    println!("  {:?} - Learns from process", SwarmRole::Observer);
    println!();
    println!("Swarms enable:");
    println!("  • Parallel execution across many agents");
    println!("  • Consensus building on complex decisions");
    println!("  • Emergent intelligence from collaboration");
    println!("  • Fault tolerance through redundancy");
    println!();
}

fn demo_agent_communication() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 8. AGENT-TO-AGENT COMMUNICATION                                       │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();
    
    // Different message types
    let messages: Vec<(&str, MessageContent)> = vec![
        ("Query", MessageContent::Query(SemanticQuery {
            query: "What do you know about quantum computing?".to_string(),
            output_type: OutputType::Text,
            context: vec![],
            confidence_threshold: 0.7,
        })),
        ("Response", MessageContent::Response {
            data: serde_json::json!({
                "summary": "Quantum computing uses qubits...",
                "key_concepts": ["superposition", "entanglement"]
            }),
            confidence: 0.92,
        }),
        ("Action Request", MessageContent::ActionRequest(Box::new(
            Action::Navigate(ResourceLocator::semantic("quantum papers"))
        ))),
        ("Knowledge Share", MessageContent::KnowledgeShare {
            topic: "Quantum Error Correction".to_string(),
            knowledge: serde_json::json!({
                "methods": ["surface codes", "topological codes"],
                "importance": "critical for fault-tolerant QC"
            }),
        }),
        ("Swarm Invite", MessageContent::SwarmInvite(SwarmTask {
            id: Uuid::new_v4(),
            description: "Research quantum algorithms".to_string(),
            goal: Box::new(Goal::Learn {
                topic: "Quantum Algorithms".to_string(),
                depth: spine_agentic::LearningDepth::Expert,
            }),
            min_members: 3,
            max_members: 10,
            required_capabilities: vec![AgentCapability::ContentExtraction],
            deadline: None,
        })),
        ("Trust Update", MessageContent::TrustUpdate {
            level: TrustLevel::Trusted,
            reason: "Consistently accurate information provided".to_string(),
        }),
    ];
    
    println!("Message types in agentic communication:");
    for (name, _content) in messages {
        println!("  • {}", name);
    }
    println!();
    println!("Features:");
    println!("  • Messages can include latent encodings for semantic matching");
    println!("  • Thread-based conversations with reply tracking");
    println!("  • Trust propagation through the network");
    println!("  • Swarm formation through invitations");
    println!();
}
