//! Cognitive Agent Demo - Advanced Reasoning and Memory
//!
//! Demonstrates:
//! - Logical reasoning with inference rules
//! - Semantic memory with associations
//! - Hierarchical goal decomposition
//! - Multi-party negotiation
//! - Resource management and allocation

use chrono::Utc;
use spine_agentic::*;
use std::collections::HashMap;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                       COGNITIVE AGENT DEMO                         ");
    println!("═══════════════════════════════════════════════════════════════════\n");

    // Demo 1: Reasoning Engine
    demo_reasoning();

    // Demo 2: Semantic Memory
    demo_semantic_memory();

    // Demo 3: Goal Decomposition
    demo_goal_decomposition();

    // Demo 4: Agent Negotiation
    demo_negotiation();

    // Demo 5: Resource Management
    demo_resource_management();

    println!("\n═══════════════════════════════════════════════════════════════════");
    println!("                    COGNITIVE DEMO COMPLETE                         ");
    println!("═══════════════════════════════════════════════════════════════════");
}

fn demo_reasoning() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                  DEMO 1: REASONING ENGINE                       │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let engine = ReasoningEngine::new();

    // Assert facts about the world
    let facts = vec![
        ("is_agent", vec!["Alice"], "Alice is an agent"),
        ("is_agent", vec!["Bob"], "Bob is an agent"),
        (
            "has_capability",
            vec!["Alice", "research"],
            "Alice can research",
        ),
        (
            "has_capability",
            vec!["Alice", "analysis"],
            "Alice can analyze",
        ),
        ("has_capability", vec!["Bob", "coding"], "Bob can code"),
        ("trusts", vec!["Alice", "Bob"], "Alice trusts Bob"),
    ];

    println!("  Asserting facts:");
    for (predicate, args, description) in facts {
        let fact = Fact {
            id: format!(
                "fact-{}",
                Uuid::new_v4().to_string().split('-').next().unwrap()
            ),
            predicate: predicate.to_string(),
            arguments: args
                .iter()
                .map(|a| FactValue::String(a.to_string()))
                .collect(),
            confidence: 1.0,
            source: FactSource::Observation,
            timestamp: Utc::now(),
        };
        engine.assert_fact(fact);
        println!("    • {}", description);
    }

    // Add inference rules
    println!("\n  Adding inference rules:");

    // Rule: If X has research and analysis, X is a data scientist
    let data_scientist_rule = InferenceRule {
        id: "data-scientist-rule".to_string(),
        name: "Data Scientist Inference".to_string(),
        conditions: vec![
            RuleCondition {
                predicate: "has_capability".to_string(),
                bindings: vec![
                    BindingPattern::Variable("X".to_string()),
                    BindingPattern::Constant(FactValue::String("research".to_string())),
                ],
                negated: false,
            },
            RuleCondition {
                predicate: "has_capability".to_string(),
                bindings: vec![
                    BindingPattern::Variable("X".to_string()),
                    BindingPattern::Constant(FactValue::String("analysis".to_string())),
                ],
                negated: false,
            },
        ],
        conclusion: RuleConclusion {
            predicate: "is_data_scientist".to_string(),
            arguments: vec![BindingPattern::Variable("X".to_string())],
        },
        confidence_factor: 0.9,
        priority: 1,
    };
    engine.add_rule(data_scientist_rule);
    println!(
        "    • has_capability(X, research) ∧ has_capability(X, analysis) → is_data_scientist(X)"
    );

    // Rule: If X trusts Y and Y is an agent, X can delegate to Y
    let delegation_rule = InferenceRule {
        id: "delegation-rule".to_string(),
        name: "Delegation Inference".to_string(),
        conditions: vec![
            RuleCondition {
                predicate: "trusts".to_string(),
                bindings: vec![
                    BindingPattern::Variable("X".to_string()),
                    BindingPattern::Variable("Y".to_string()),
                ],
                negated: false,
            },
            RuleCondition {
                predicate: "is_agent".to_string(),
                bindings: vec![BindingPattern::Variable("Y".to_string())],
                negated: false,
            },
        ],
        conclusion: RuleConclusion {
            predicate: "can_delegate".to_string(),
            arguments: vec![
                BindingPattern::Variable("X".to_string()),
                BindingPattern::Variable("Y".to_string()),
            ],
        },
        confidence_factor: 0.85,
        priority: 2,
    };
    engine.add_rule(delegation_rule);
    println!("    • trusts(X, Y) ∧ is_agent(Y) → can_delegate(X, Y)");

    // Run inference
    println!("\n  Running forward chaining inference...");
    let inferences = engine.infer();

    println!("  New inferences:");
    for inference in &inferences {
        let args: Vec<String> = inference
            .result
            .arguments
            .iter()
            .map(|v| match v {
                FactValue::String(s) => s.clone(),
                _ => "?".to_string(),
            })
            .collect();
        println!(
            "    ✓ {}({}) [confidence: {:.2}]",
            inference.result.predicate,
            args.join(", "),
            inference.confidence
        );
    }

    // Query the knowledge base
    println!("\n  Querying: Who is a data scientist?");
    let results = engine.query(
        "is_data_scientist",
        &[BindingPattern::Variable("X".to_string())],
    );
    for (fact, bindings) in results {
        if let Some(FactValue::String(name)) = bindings.get("X") {
            println!("    → {} (confidence: {:.2})", name, fact.confidence);
        }
    }

    println!();
}

fn demo_semantic_memory() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                  DEMO 2: SEMANTIC MEMORY                        │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let memory = SemanticMemory::new();

    // Learn concepts
    println!("  Learning concepts:");
    memory.learn_concept(
        "Agent",
        "An autonomous entity that can perceive and act",
        vec![],
    );
    memory.learn_concept(
        "WebAgent",
        "An agent that navigates the web",
        vec!["Agent".to_string()],
    );
    memory.learn_concept(
        "ResearchAgent",
        "An agent specialized in research tasks",
        vec!["WebAgent".to_string()],
    );
    println!("    • Agent → WebAgent → ResearchAgent (hierarchy)");

    // Create associations
    println!("\n  Creating associations:");
    memory.associate("WebAgent", "Navigation", RelationType::HasA, 0.9);
    memory.associate("ResearchAgent", "Analysis", RelationType::HasA, 0.95);
    memory.associate("Navigation", "URL", RelationType::PartOf, 0.8);
    memory.associate("Analysis", "Extraction", RelationType::SimilarTo, 0.7);
    println!("    • WebAgent --has-a--> Navigation (0.9)");
    println!("    • ResearchAgent --has-a--> Analysis (0.95)");
    println!("    • Navigation --part-of--> URL (0.8)");
    println!("    • Analysis --similar-to--> Extraction (0.7)");

    // Store episodic memories
    println!("\n  Storing episodic memories:");
    let episodes = vec![
        ("Searched for AI papers on arxiv.org", 0.8),
        ("Extracted data from market research report", 0.9),
        ("Collaborated with CodingAgent on implementation", 0.7),
        ("Completed quarterly analysis successfully", 0.95),
    ];

    for (description, importance) in episodes {
        let id = memory.remember(
            serde_json::json!({ "description": description }),
            MemoryContext {
                location: Some("research_session".to_string()),
                task: Some("data_analysis".to_string()),
                agents_involved: vec![],
                tags: vec!["research".to_string()],
            },
            importance,
        );
        println!(
            "    • {} (importance: {:.1}) → {}",
            description,
            importance,
            &id.to_string()[..8]
        );
    }

    // Spread activation
    println!("\n  Spreading activation from 'ResearchAgent':");
    let activations = memory.spread_activation("ResearchAgent", 2);
    let mut sorted: Vec<_> = activations.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());

    for (concept, activation) in sorted.iter().take(5) {
        let bar_len = ((**activation) * 20.0) as usize;
        let bar = "█".repeat(bar_len);
        println!("    {} {:.<20} {:.3}", bar, concept, activation);
    }

    // Working memory
    println!("\n  Working memory (capacity 7):");
    memory.focus("current_research");
    memory.attend("task-1", serde_json::json!({"action": "analyze_data"}));
    memory.attend("task-2", serde_json::json!({"action": "extract_insights"}));
    memory.attend("task-3", serde_json::json!({"action": "generate_report"}));

    if let Some(wm) = memory.get_working_memory() {
        println!("    Focus: current_research");
        println!("    Items: {} (max 7)", wm.len());
        for item in &wm {
            println!("      • {} (activation: {:.2})", item.id, item.activation);
        }
    }

    // Recall recent memories
    println!("\n  Recalling recent memories:");
    let recent = memory.recall_recent(3);
    for (i, episode) in recent.iter().enumerate() {
        if let Some(desc) = episode.content.get("description") {
            println!(
                "    {}. {} (importance: {:.1})",
                i + 1,
                desc,
                episode.importance
            );
        }
    }

    println!();
}

fn demo_goal_decomposition() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                DEMO 3: GOAL DECOMPOSITION                       │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let decomposer = GoalDecomposer::new();

    // Create root goal
    let root_goal = decomposer.create_goal(
        "Complete Market Research Report",
        "Analyze market trends and produce a comprehensive report",
        GoalType::Achievement {
            target_state: "report_delivered".to_string(),
        },
    );
    println!("  Created root goal: Complete Market Research Report");

    // Decompose into subgoals
    let phase1_goals = decomposer.decompose(
        root_goal,
        vec![
            (
                "Gather Data".to_string(),
                GoalType::Achievement {
                    target_state: "data_collected".to_string(),
                },
            ),
            (
                "Analyze Trends".to_string(),
                GoalType::Achievement {
                    target_state: "trends_identified".to_string(),
                },
            ),
            (
                "Write Report".to_string(),
                GoalType::Procedure {
                    steps: vec![
                        "outline".to_string(),
                        "draft".to_string(),
                        "review".to_string(),
                    ],
                },
            ),
        ],
    );
    println!("  Decomposed into 3 phases:");
    println!("    1. Gather Data");
    println!("    2. Analyze Trends");
    println!("    3. Write Report");

    // Further decompose data gathering
    let gather_data_id = phase1_goals[0];
    let data_subgoals = decomposer.decompose(
        gather_data_id,
        vec![
            (
                "Collect Market Prices".to_string(),
                GoalType::Query {
                    question: "What are current market prices?".to_string(),
                },
            ),
            (
                "Survey Competitors".to_string(),
                GoalType::Query {
                    question: "Who are the key competitors?".to_string(),
                },
            ),
            (
                "Analyze Customer Feedback".to_string(),
                GoalType::Optimization {
                    metric: "sentiment_score".to_string(),
                    direction: OptimizationDirection::Maximize,
                },
            ),
        ],
    );
    println!("\n  'Gather Data' decomposed into 3 tasks:");
    println!("    1.1 Collect Market Prices");
    println!("    1.2 Survey Competitors");
    println!("    1.3 Analyze Customer Feedback");

    // Get leaf goals (actionable items)
    let leaves = decomposer.get_leaf_goals();
    println!("\n  Leaf goals (actionable):");
    for goal in leaves.iter().take(5) {
        let status = match goal.status {
            GoalStatus::Pending => "⏳",
            GoalStatus::Active => "🔄",
            GoalStatus::Achieved => "✅",
            _ => "❓",
        };
        println!(
            "    {} {} (progress: {:.0}%)",
            status,
            goal.name,
            goal.progress * 100.0
        );
    }

    // Simulate progress updates
    println!("\n  Updating progress:");
    decomposer.update_progress(data_subgoals[0], 1.0);
    println!("    ✓ 'Collect Market Prices' completed");
    decomposer.update_progress(data_subgoals[1], 0.5);
    println!("    → 'Survey Competitors' at 50%");

    // Show hierarchy
    if let Some(tree) = decomposer.get_hierarchy(root_goal) {
        println!("\n  Goal Hierarchy:");
        print_goal_tree(&tree, 2);
    }

    println!();
}

fn print_goal_tree(tree: &GoalTree, indent: usize) {
    let prefix = " ".repeat(indent);
    let progress = (tree.goal.progress * 100.0) as u32;
    let status_icon = if tree.goal.progress >= 1.0 {
        "✅"
    } else if tree.goal.progress > 0.0 {
        "🔄"
    } else {
        "⏳"
    };

    println!(
        "{}{}─ {} [{}%]",
        prefix, status_icon, tree.goal.name, progress
    );

    for child in &tree.children {
        print_goal_tree(child, indent + 4);
    }
}

fn demo_negotiation() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                  DEMO 4: AGENT NEGOTIATION                      │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let protocol = NegotiationProtocol::new();

    // Create negotiating agents
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();
    let charlie = Uuid::new_v4();

    println!("  Participants:");
    println!("    • Alice (Research Agent)");
    println!("    • Bob (Coding Agent)");
    println!("    • Charlie (Data Agent)");

    // Set negotiation strategies
    protocol.set_strategy(
        alice,
        NegotiationStrategy::Cooperative {
            concession_rate: 0.1,
        },
    );
    protocol.set_strategy(bob, NegotiationStrategy::TitForTat);
    protocol.set_strategy(
        charlie,
        NegotiationStrategy::BATNA {
            best_alternative_value: 50.0,
        },
    );

    println!("\n  Strategies assigned:");
    println!("    • Alice: Cooperative (concession rate 0.1)");
    println!("    • Bob: Tit-for-Tat");
    println!("    • Charlie: BATNA (alternative value: 50)");

    // Initiate negotiation
    let negotiation_id = protocol.initiate(
        "Task Allocation for Q1 Project",
        vec![alice, bob, charlie],
        NegotiationRules {
            max_rounds: 5,
            timeout_per_round_secs: 60,
            allow_coalitions: true,
            allow_side_payments: false,
            voting_threshold: 0.66,
        },
    );

    println!("\n  Negotiation started: Task Allocation for Q1 Project");
    println!("    Max rounds: 5, Threshold: 66%");

    // Alice makes initial proposal
    let mut utilities = HashMap::new();
    utilities.insert(alice, 80.0);
    utilities.insert(bob, 60.0);
    utilities.insert(charlie, 70.0);

    let mut terms = HashMap::new();
    terms.insert("research_hours".to_string(), serde_json::json!(40));
    terms.insert("coding_hours".to_string(), serde_json::json!(60));
    terms.insert("data_hours".to_string(), serde_json::json!(30));

    let proposal = Proposal {
        id: Uuid::new_v4(),
        proposer: alice,
        terms,
        utility_claims: utilities,
        timestamp: Utc::now(),
    };

    protocol.propose(negotiation_id, proposal).unwrap();
    println!("\n  Alice proposes:");
    println!("    • Research hours: 40");
    println!("    • Coding hours: 60");
    println!("    • Data hours: 30");

    // Bob accepts
    protocol
        .respond(
            negotiation_id,
            ProposalResponse {
                responder: bob,
                response_type: ResponseType::Accept,
                counter_proposal: None,
                utility: 60.0,
            },
        )
        .unwrap();
    println!("\n  Bob: ACCEPTS (utility: 60)");

    // Charlie makes counter-proposal
    let mut counter_utilities = HashMap::new();
    counter_utilities.insert(alice, 75.0);
    counter_utilities.insert(bob, 55.0);
    counter_utilities.insert(charlie, 85.0);

    let mut counter_terms = HashMap::new();
    counter_terms.insert("research_hours".to_string(), serde_json::json!(35));
    counter_terms.insert("coding_hours".to_string(), serde_json::json!(50));
    counter_terms.insert("data_hours".to_string(), serde_json::json!(45));

    let counter = Proposal {
        id: Uuid::new_v4(),
        proposer: charlie,
        terms: counter_terms,
        utility_claims: counter_utilities,
        timestamp: Utc::now(),
    };

    protocol
        .respond(
            negotiation_id,
            ProposalResponse {
                responder: charlie,
                response_type: ResponseType::Counter,
                counter_proposal: Some(counter),
                utility: 70.0,
            },
        )
        .unwrap();
    println!("  Charlie: COUNTER-PROPOSES");
    println!("    • Research hours: 35 (-5)");
    println!("    • Coding hours: 50 (-10)");
    println!("    • Data hours: 45 (+15)");

    // Check status
    let status = protocol.get_status(negotiation_id);
    println!("\n  Negotiation status: {:?}", status);

    // Find pareto-optimal solutions
    let pareto = protocol.find_pareto_optimal(negotiation_id);
    println!("  Pareto-optimal proposals: {}", pareto.len());

    println!();
}

fn demo_resource_management() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                DEMO 5: RESOURCE MANAGEMENT                      │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let manager = ResourceManager::new();

    // Register resources
    let resources = vec![
        (
            "compute",
            "GPU Compute Hours",
            ResourceType::Compute,
            1000.0,
            "hours",
        ),
        (
            "memory",
            "Memory Allocation",
            ResourceType::Memory,
            64.0,
            "GB",
        ),
        (
            "tokens",
            "API Tokens",
            ResourceType::Tokens,
            100000.0,
            "tokens",
        ),
        (
            "credits",
            "Agent Credits",
            ResourceType::Credits,
            500.0,
            "credits",
        ),
    ];

    println!("  Registering resources:");
    for (id, name, rtype, capacity, unit) in resources {
        manager.register_resource(Resource {
            id: id.to_string(),
            name: name.to_string(),
            resource_type: rtype,
            total_capacity: capacity,
            available: capacity,
            unit: unit.to_string(),
            renewable: id == "tokens",
            renewal_rate: if id == "tokens" { Some(1000.0) } else { None },
        });
        println!("    • {}: {:.0} {} available", name, capacity, unit);
    }

    // Create agent quotas
    let research_agent = Uuid::new_v4();
    let coding_agent = Uuid::new_v4();

    let mut research_limits = HashMap::new();
    research_limits.insert("compute".to_string(), 200.0);
    research_limits.insert("tokens".to_string(), 50000.0);
    research_limits.insert("credits".to_string(), 150.0);

    manager.set_quota(research_agent, research_limits, QuotaPeriod::Daily);

    let mut coding_limits = HashMap::new();
    coding_limits.insert("compute".to_string(), 300.0);
    coding_limits.insert("memory".to_string(), 32.0);
    coding_limits.insert("credits".to_string(), 200.0);

    manager.set_quota(coding_agent, coding_limits, QuotaPeriod::Daily);

    println!("\n  Agent quotas set:");
    println!("    • ResearchAgent: 200 compute, 50k tokens, 150 credits (daily)");
    println!("    • CodingAgent: 300 compute, 32 GB memory, 200 credits (daily)");

    // Allocate resources
    println!("\n  Allocating resources:");

    match manager.allocate(research_agent, "compute", 50.0, AllocationPriority::High) {
        Ok(_alloc) => println!("    ✓ ResearchAgent: 50 compute hours allocated (high priority)"),
        Err(e) => println!("    ✗ ResearchAgent compute: {}", e),
    }

    match manager.allocate(
        research_agent,
        "tokens",
        10000.0,
        AllocationPriority::Normal,
    ) {
        Ok(_) => println!("    ✓ ResearchAgent: 10,000 tokens allocated"),
        Err(e) => println!("    ✗ ResearchAgent tokens: {}", e),
    }

    match manager.allocate(coding_agent, "memory", 16.0, AllocationPriority::High) {
        Ok(_) => println!("    ✓ CodingAgent: 16 GB memory allocated"),
        Err(e) => println!("    ✗ CodingAgent memory: {}", e),
    }

    // Record usage
    manager.record_usage(research_agent, "tokens", 5000.0, "web_search");
    manager.record_usage(research_agent, "tokens", 3000.0, "summarization");

    // Check availability
    println!("\n  Resource availability:");
    for resource_id in ["compute", "memory", "tokens", "credits"] {
        if let Some(available) = manager.get_availability(resource_id) {
            println!("    • {}: {:.0} available", resource_id, available);
        }
    }

    // Get usage summary
    let usage = manager.get_usage(research_agent);
    println!("\n  ResearchAgent usage:");
    for (resource, amount) in &usage {
        println!("    • {}: {:.0} used", resource, amount);
    }

    // Release resources
    println!("\n  Releasing resources:");
    match manager.release(research_agent, "compute") {
        Ok(amount) => println!("    ✓ Released {:.0} compute hours", amount),
        Err(e) => println!("    ✗ Release failed: {}", e),
    }

    // Resource summary
    let summary = manager.get_summary();
    println!("\n  System Summary:");
    println!("    Total resources: {}", summary.total_resources);
    println!("    Total capacity: {:.0}", summary.total_capacity);
    println!("    Utilization: {:.1}%", summary.utilization * 100.0);
    println!("    Active allocations: {}", summary.active_allocations);

    println!();
}
