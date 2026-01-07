//! Infrastructure Demo - Advanced Agent Runtime Features
//! 
//! Demonstrates:
//! - Agent versioning and hot migrations
//! - Semantic routing mesh
//! - Distributed consensus protocols
//! - Agent introspection and debugging
//! - Policy engine for access control
//! - Event sourcing for audit trails
//! - Agent federation across networks

use spine_agentic::*;
use uuid::Uuid;
use chrono::Utc;
use std::time::Duration;
use std::sync::Arc;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                    HYPERLIGHT INFRASTRUCTURE DEMO                  ");
    println!("═══════════════════════════════════════════════════════════════════\n");

    // Demo 1: Agent Versioning & Migrations
    demo_versioning().await;
    
    // Demo 2: Semantic Routing Mesh
    demo_semantic_routing().await;
    
    // Demo 3: Distributed Consensus
    demo_consensus().await;
    
    // Demo 4: Agent Introspection
    demo_introspection().await;
    
    // Demo 5: Policy Engine
    demo_policy_engine().await;
    
    // Demo 6: Event Sourcing
    demo_event_sourcing().await;
    
    // Demo 7: Agent Federation
    demo_federation().await;
    
    println!("\n═══════════════════════════════════════════════════════════════════");
    println!("                    INFRASTRUCTURE DEMO COMPLETE                    ");
    println!("═══════════════════════════════════════════════════════════════════");
}

async fn demo_versioning() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│              DEMO 1: AGENT VERSIONING & MIGRATIONS              │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");
    
    let controller = MigrationController::new();
    
    // Define version progression
    let v1 = AgentVersion::new(1, 0, 0);
    let v1_1 = AgentVersion::new(1, 1, 0);
    let v2 = AgentVersion::new(2, 0, 0);
    
    println!("  Version progression: {} -> {} -> {}", v1, v1_1, v2);
    println!("  Compatible: {} <-> {} = {}", v1, v1_1, v1.is_compatible_with(&v1_1));
    println!("  Compatible: {} <-> {} = {}", v1, v2, v1.is_compatible_with(&v2));
    
    // Register migration
    let migration = Migration {
        id: "upgrade-v1-to-v1.1".to_string(),
        from_version: v1.clone(),
        to_version: v1_1.clone(),
        description: "Add knowledge management capability".to_string(),
        reversible: true,
        steps: vec![
            MigrationStep::AddCapability(AgentCapability::KnowledgeManagement),
            MigrationStep::TransformKnowledge { 
                transform_fn: "index_all_entities".to_string() 
            },
            MigrationStep::UpdateBehavior {
                old_behavior: "simple_search".to_string(),
                new_behavior: "semantic_search".to_string(),
            },
        ],
    };
    
    controller.register_migration(migration);
    println!("\n  Registered migration: upgrade-v1-to-v1.1");
    
    // Simulate agent upgrade
    let agent_id = Uuid::new_v4();
    let state = serde_json::json!({
        "name": "ResearchAgent",
        "knowledge_count": 42,
        "last_active": "2025-01-01T00:00:00Z"
    });
    
    // Create snapshot before migration
    let snapshot = controller.snapshot(agent_id, v1.clone(), state);
    println!("  Created snapshot: {} @ v{}", 
             &snapshot.id.to_string()[..8], 
             snapshot.version);
    
    // Find migration path
    let path = controller.find_migration_path(&v1, &v1_1);
    println!("  Migration path: {} steps", path.len());
    
    // Apply migrations
    for migration in &path {
        println!("\n  Applying migration: {}", migration.id);
        controller.apply_migration(agent_id, migration).await.unwrap();
    }
    
    println!("\n  ✓ Agent upgraded from v{} to v{}", v1, v1_1);
    
    println!();
}

async fn demo_semantic_routing() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                 DEMO 2: SEMANTIC ROUTING MESH                   │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");
    
    let router = SemanticRouter::new();
    
    // Create agents
    let research_agent = Uuid::new_v4();
    let coding_agent = Uuid::new_v4();
    let data_agent = Uuid::new_v4();
    
    // Add semantic routes
    router.add_route(SemanticRoute {
        id: Uuid::new_v4(),
        pattern: SemanticPattern::Topic(vec!["research".into(), "paper".into(), "analysis".into()]),
        destination: RouteDestination::Agent(research_agent),
        priority: 10,
        filters: vec![],
        transforms: vec![],
    });
    
    router.add_route(SemanticRoute {
        id: Uuid::new_v4(),
        pattern: SemanticPattern::Topic(vec!["code".into(), "programming".into(), "implementation".into()]),
        destination: RouteDestination::Agent(coding_agent),
        priority: 10,
        filters: vec![],
        transforms: vec![],
    });
    
    router.add_route(SemanticRoute {
        id: Uuid::new_v4(),
        pattern: SemanticPattern::Embedding { 
            center: vec![0.8, 0.2, -0.3], 
            radius: 0.5 
        },
        destination: RouteDestination::Pool { 
            agents: vec![data_agent], 
            strategy: LoadBalanceStrategy::LeastConnections 
        },
        priority: 5,
        filters: vec![RouteFilter::TrustLevel(TrustLevel::Verified)],
        transforms: vec![MessageTransform::AddMetadata { 
            key: "routed_via".into(), 
            value: "semantic_mesh".into() 
        }],
    });
    
    println!("  Registered 3 semantic routes:");
    println!("    • Topic-based → ResearchAgent (research|paper|analysis)");
    println!("    • Topic-based → CodingAgent (code|programming|implementation)");
    println!("    • Embedding-based → DataPool (load balanced)");
    
    // Subscribe to patterns
    let sub_id = router.subscribe(
        coding_agent,
        SemanticPattern::Topic(vec!["bug".into(), "error".into(), "fix".into()]),
        "handle_bug_report"
    );
    println!("\n  CodingAgent subscribed to bug reports: {}", &sub_id.to_string()[..8]);
    
    // Route messages
    let messages = vec![
        ("research paper on neural networks", vec!["research".to_string()]),
        ("implement the algorithm in Rust", vec!["code".to_string()]),
        ("bug in the parser module", vec!["bug".to_string()]),
    ];
    
    println!("\n  Routing messages:");
    for (content, _) in messages {
        let msg = RoutedMessage {
            id: Uuid::new_v4(),
            source: Uuid::new_v4(),
            content: serde_json::json!({ "text": content }),
            embedding: None,
            routed_at: Utc::now(),
            route_path: vec![],
            ttl: 30,
        };
        
        let destinations = router.route(msg).await;
        println!("    \"{}\" → {} destinations", content, destinations.len());
    }
    
    println!();
}

async fn demo_consensus() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│               DEMO 3: DISTRIBUTED CONSENSUS                     │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");
    
    let engine = ConsensusEngine::new();
    
    // Create participants
    let participants: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    println!("  Created 5 consensus participants");
    
    // Create proposal
    let proposal_id = engine.propose(
        participants[0],
        "Select next task priority",
        serde_json::json!({
            "task": "Optimize neural encoder",
            "priority": "high",
            "deadline": "2025-02-01"
        }),
        QuorumRequirement::SuperMajority,
        participants.clone(),
        60,
    );
    
    println!("\n  Proposal created: {}", &proposal_id.to_string()[..8]);
    println!("  Topic: Select next task priority");
    println!("  Quorum: SuperMajority (>66%)");
    
    // Cast votes
    let votes = vec![
        (participants[0], VoteDecision::Accept, 1.0, Some("Strongly agree")),
        (participants[1], VoteDecision::Accept, 1.2, Some("Agree with high priority")),
        (participants[2], VoteDecision::Accept, 0.8, None),
        (participants[3], VoteDecision::Reject, 1.0, Some("Prefer different task")),
        (participants[4], VoteDecision::Conditional { 
            conditions: vec!["If resources available".into()] 
        }, 1.1, None),
    ];
    
    println!("\n  Voting:");
    for (voter, decision, weight, reasoning) in votes {
        engine.vote(proposal_id, voter, decision.clone(), weight, reasoning).unwrap();
        let decision_str = match &decision {
            VoteDecision::Accept => "Accept",
            VoteDecision::Reject => "Reject",
            VoteDecision::Abstain => "Abstain",
            VoteDecision::Conditional { .. } => "Conditional",
        };
        println!("    Participant {} voted: {} (weight: {:.1})", 
                 &voter.to_string()[..4], decision_str, weight);
    }
    
    // Check consensus
    let status = engine.check_consensus(proposal_id);
    println!("\n  Consensus status: {:?}", status);
    
    if status == ConsensusStatus::Accepted || status == ConsensusStatus::Voting {
        // Commit decision
        let committed = engine.commit(proposal_id).unwrap();
        println!("  ✓ Decision committed at {}", committed.committed_at.format("%H:%M:%S"));
        println!("    Accept votes: {}", committed.vote_summary.accept_count);
        println!("    Reject votes: {}", committed.vote_summary.reject_count);
        println!("    Accept weight: {:.1}/{:.1}", 
                 committed.vote_summary.accept_weight,
                 committed.vote_summary.total_weight);
    }
    
    println!();
}

async fn demo_introspection() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│              DEMO 4: AGENT INTROSPECTION & DEBUGGING            │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");
    
    let tracer = AgentTracer::new();
    let agent_id = Uuid::new_v4();
    
    // Start trace
    let session_id = tracer.start_trace(agent_id);
    println!("  Started trace session: {}", &session_id.to_string()[..8]);
    
    // Add breakpoint
    let bp_id = tracer.add_breakpoint(
        agent_id,
        BreakpointCondition::OnAction("fetch_data".to_string()),
        BreakpointAction::Log { message: "Data fetch started".into() }
    );
    println!("  Added breakpoint: {} (on fetch_data action)", &bp_id.to_string()[..8]);
    
    // Add state watcher
    let watcher_id = tracer.watch(agent_id, "knowledge_count", "on_knowledge_change");
    println!("  Added watcher: {} (on knowledge_count)", &watcher_id.to_string()[..8]);
    
    // Simulate execution with trace events
    let events = vec![
        (TraceEventType::AgentStart, serde_json::json!({"agent": "ResearchAgent"})),
        (TraceEventType::IntentionCreated { goal: "Analyze market data".into() }, serde_json::json!({})),
        (TraceEventType::PlanningStarted, serde_json::json!({})),
        (TraceEventType::PlanningCompleted { steps: 3 }, serde_json::json!({"duration_ms": 15})),
        (TraceEventType::ActionStarted { action: "fetch_data".into() }, serde_json::json!({"source": "api.market.com"})),
        (TraceEventType::ActionCompleted { result: "200 OK".into() }, serde_json::json!({"records": 1500})),
        (TraceEventType::KnowledgeUpdate { node_id: "market-2025".into() }, serde_json::json!({})),
        (TraceEventType::MessageSent { to: Uuid::new_v4() }, serde_json::json!({"type": "analysis_result"})),
    ];
    
    println!("\n  Recording trace events:");
    for (event_type, data) in events {
        // Check breakpoints
        if let Some(action) = tracer.check_breakpoints(agent_id, &event_type) {
            match action {
                BreakpointAction::Log { message } => println!("    [BP] {}", message),
                BreakpointAction::Pause => println!("    [BP] Execution paused"),
                _ => {}
            }
        }
        
        let event_id = tracer.record(session_id, event_type.clone(), data);
        if event_id.is_some() {
            let type_str = match &event_type {
                TraceEventType::AgentStart => "AgentStart",
                TraceEventType::IntentionCreated { .. } => "IntentionCreated",
                TraceEventType::PlanningStarted => "PlanningStarted",
                TraceEventType::PlanningCompleted { .. } => "PlanningCompleted",
                TraceEventType::ActionStarted { .. } => "ActionStarted",
                TraceEventType::ActionCompleted { .. } => "ActionCompleted",
                TraceEventType::KnowledgeUpdate { .. } => "KnowledgeUpdate",
                TraceEventType::MessageSent { .. } => "MessageSent",
                _ => "Other",
            };
            println!("    • {}", type_str);
        }
    }
    
    // Get trace summary
    if let Some(summary) = tracer.summarize(session_id) {
        println!("\n  Trace Summary:");
        println!("    Duration: {} ms", summary.duration_ms);
        println!("    Total events: {}", summary.event_count);
        println!("    Actions: {}", summary.action_count);
        println!("    Messages: {}", summary.message_count);
        println!("    Errors: {}", summary.error_count);
    }
    
    println!();
}

async fn demo_policy_engine() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                   DEMO 5: POLICY ENGINE                         │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");
    
    let engine = PolicyEngine::new();
    
    // Add policies
    let admin_policy = Policy {
        id: "admin-access".into(),
        name: "Admin Full Access".into(),
        description: "Administrators have full access to all resources".into(),
        rules: vec![PolicyRule {
            id: "admin-rule".into(),
            subjects: vec![SubjectMatcher::InGroup("admins".into())],
            resources: vec![ResourceMatcher::Any],
            actions: vec!["*".into()],
            conditions: vec![],
        }],
        priority: 100,
        effect: PolicyEffect::Allow,
        enabled: true,
    };
    
    let knowledge_policy = Policy {
        id: "knowledge-access".into(),
        name: "Knowledge Base Access".into(),
        description: "Trusted agents can read knowledge base".into(),
        rules: vec![PolicyRule {
            id: "knowledge-rule".into(),
            subjects: vec![SubjectMatcher::TrustLevel(TrustLevel::Trusted)],
            resources: vec![ResourceMatcher::Path("/knowledge".into())],
            actions: vec!["read".into(), "search".into()],
            conditions: vec![],
        }],
        priority: 50,
        effect: PolicyEffect::Allow,
        enabled: true,
    };
    
    let sensitive_policy = Policy {
        id: "sensitive-deny".into(),
        name: "Deny Sensitive Access".into(),
        description: "Block access to sensitive resources for unverified agents".into(),
        rules: vec![PolicyRule {
            id: "sensitive-rule".into(),
            subjects: vec![SubjectMatcher::TrustLevel(TrustLevel::Unknown)],
            resources: vec![ResourceMatcher::Tagged("sensitive".into())],
            actions: vec!["*".into()],
            conditions: vec![],
        }],
        priority: 90,
        effect: PolicyEffect::Deny,
        enabled: true,
    };
    
    engine.add_policy(admin_policy);
    engine.add_policy(knowledge_policy);
    engine.add_policy(sensitive_policy);
    
    println!("  Registered 3 policies:");
    println!("    • admin-access: Admins have full access (priority: 100)");
    println!("    • knowledge-access: Trusted agents can read knowledge (priority: 50)");
    println!("    • sensitive-deny: Block unverified from sensitive (priority: 90)");
    
    // Evaluate access requests
    let test_cases = vec![
        ("Admin reading config", Uuid::new_v4(), "/config/settings", "read", 
         EvaluationContext { groups: vec!["admins".into()], ..Default::default() }),
        
        ("Trusted agent reading knowledge", Uuid::new_v4(), "/knowledge/facts", "read",
         EvaluationContext { trust_level: TrustLevel::Trusted, ..Default::default() }),
        
        ("Unknown agent accessing sensitive", Uuid::new_v4(), "/data/sensitive", "read",
         EvaluationContext { 
             trust_level: TrustLevel::Unknown, 
             resource_tags: vec!["sensitive".into()],
             ..Default::default() 
         }),
        
        ("Verified agent writing data", Uuid::new_v4(), "/data/reports", "write",
         EvaluationContext { trust_level: TrustLevel::Verified, ..Default::default() }),
    ];
    
    println!("\n  Evaluating access requests:");
    for (description, subject, resource, action, context) in test_cases {
        let result = engine.evaluate(subject, resource, action, &context);
        let effect_symbol = match result.decision {
            PolicyEffect::Allow => "✓",
            PolicyEffect::Deny => "✗",
            PolicyEffect::RequireApproval => "?",
            PolicyEffect::Log => "→",
        };
        println!("    {} {} - {:?}", effect_symbol, description, result.decision);
    }
    
    println!();
}

async fn demo_event_sourcing() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                   DEMO 6: EVENT SOURCING                        │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");
    
    let store = EventStore::new();
    let agent_id = Uuid::new_v4();
    
    // Create projection
    store.create_projection(
        "agent_stats",
        vec!["AgentCreated".into(), "TaskCompleted".into(), "KnowledgeAdded".into()],
        serde_json::json!({ "task_count": 0, "knowledge_count": 0 })
    );
    
    println!("  Created projection: agent_stats");
    
    // Append events
    let events = vec![
        ("AgentCreated", serde_json::json!({ "name": "DataAnalyzer", "version": "1.0.0" })),
        ("TaskCompleted", serde_json::json!({ "task_id": "analyze-q1", "duration_ms": 1500 })),
        ("KnowledgeAdded", serde_json::json!({ "node_id": "insight-1", "type": "pattern" })),
        ("TaskCompleted", serde_json::json!({ "task_id": "extract-data", "duration_ms": 800 })),
        ("KnowledgeAdded", serde_json::json!({ "node_id": "insight-2", "type": "anomaly" })),
        ("TaskCompleted", serde_json::json!({ "task_id": "generate-report", "duration_ms": 2000 })),
    ];
    
    println!("\n  Appending events:");
    for (i, (event_type, data)) in events.iter().enumerate() {
        let event = StoredEvent {
            id: Uuid::new_v4(),
            aggregate_id: agent_id,
            aggregate_type: "Agent".into(),
            event_type: event_type.to_string(),
            version: i as u64,
            data: data.clone(),
            metadata: EventMetadata {
                correlation_id: Some(Uuid::new_v4()),
                causation_id: None,
                actor: Some(agent_id),
                source: "infrastructure_demo".into(),
                tags: vec!["demo".into()],
            },
            timestamp: Utc::now(),
        };
        
        let version = store.append(agent_id, event);
        println!("    Event {} (v{}): {}", i + 1, version, event_type);
    }
    
    // Subscribe to events
    let sub_id = store.subscribe(
        vec!["TaskCompleted".into()],
        "update_dashboard"
    );
    println!("\n  Subscribed to TaskCompleted: {}", &sub_id.to_string()[..8]);
    
    // Load events
    let loaded = store.load(agent_id);
    println!("\n  Loaded {} events for agent", loaded.len());
    
    // Get snapshot
    if let Some(snapshot) = store.snapshot(agent_id) {
        println!("  Current snapshot: v{} @ {}", 
                 snapshot.version, 
                 snapshot.timestamp.format("%H:%M:%S"));
    }
    
    // Replay events - count manually since we can't mutate in Fn closure
    let events = store.load(agent_id);
    let task_count = events.iter().filter(|e| e.event_type == "TaskCompleted").count();
    let knowledge_count = events.iter().filter(|e| e.event_type == "KnowledgeAdded").count();
    
    println!("\n  Replay results:");
    println!("    Tasks completed: {}", task_count);
    println!("    Knowledge nodes: {}", knowledge_count);
    
    println!();
}

async fn demo_federation() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                  DEMO 7: AGENT FEDERATION                       │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");
    
    let federation = AgentFederation::new();
    
    // Register local agents
    let agents = vec![
        ("ResearchBot", vec![AgentCapability::ContentExtraction, AgentCapability::KnowledgeManagement]),
        ("CodingBot", vec![AgentCapability::CodeExecution, AgentCapability::ApiAccess]),
        ("DataBot", vec![AgentCapability::ContentExtraction, AgentCapability::AutonomousDecision]),
    ];
    
    println!("  Registering local agents:");
    for (name, capabilities) in agents {
        let agent = FederatedAgent {
            id: Uuid::new_v4(),
            did: None,
            capabilities: capabilities.clone(),
            federation: "hyperlight-local".into(),
            endpoints: vec!["wss://local.SPINE.net/agent".into()],
            trust_level: TrustLevel::Trusted,
            last_seen: Utc::now(),
        };
        federation.register_local(agent);
        println!("    • {} with {} capabilities", name, capabilities.len());
    }
    
    // Add remote registries
    let remote_registries = vec![
        ("research-net", "Academic Research Network", "wss://research.edu/registry"),
        ("enterprise-net", "Enterprise Agent Network", "wss://enterprise.corp/registry"),
    ];
    
    println!("\n  Adding remote registries:");
    for (id, name, endpoint) in remote_registries {
        let registry = RemoteRegistry {
            id: id.into(),
            name: name.into(),
            endpoint: endpoint.into(),
            protocol: "chameleon-v1".into(),
            trust_level: TrustLevel::Verified,
            agents_count: 50,
            last_sync: Utc::now(),
        };
        federation.add_remote_registry(registry);
        println!("    • {} ({})", name, endpoint);
    }
    
    // Establish trust links
    federation.establish_trust("hyperlight-local", "research-net", TrustLevel::Trusted, true);
    federation.establish_trust("hyperlight-local", "enterprise-net", TrustLevel::Verified, false);
    
    println!("\n  Established trust links:");
    println!("    • hyperlight-local ↔ research-net (Trusted, bidirectional)");
    println!("    • hyperlight-local → enterprise-net (Verified, one-way)");
    
    // Find agents across federations
    println!("\n  Finding agents with ContentExtraction capability:");
    let results = federation.find_agent(AgentCapability::ContentExtraction).await;
    println!("    Found {} local agents", results.len());
    
    // Get federation stats
    let stats = federation.stats();
    println!("\n  Federation Statistics:");
    println!("    Local agents: {}", stats.local_agents);
    println!("    Remote registries: {}", stats.remote_registries);
    println!("    Trust links: {}", stats.trust_links);
    println!("    Routing entries: {}", stats.routes);
    
    println!();
}
