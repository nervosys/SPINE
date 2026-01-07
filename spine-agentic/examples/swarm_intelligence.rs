//! # Swarm Intelligence Demo
//!
//! Demonstrates collective intelligence through agent swarms that coordinate
//! to solve complex problems no single agent could handle alone.
//!
//! Run: cargo run --example swarm_intelligence -p hyperlight-agentic

use spine_agentic::{
    create_agent_system, create_agent_with_capabilities,
    AgentCapability, Goal, SwarmTask, SwarmRole, TrustLevel,
    AgentRegistry, SwarmCoordinator, RegisteredAgent,
};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    println!("╔═══════════════════════════════════════════════════════════════════════╗");
    println!("║              SWARM INTELLIGENCE DEMONSTRATION                         ║");
    println!("║           Collective Problem-Solving Across Agents                    ║");
    println!("╚═══════════════════════════════════════════════════════════════════════╝");
    println!();

    demo_agent_registry();
    demo_swarm_formation().await;
    demo_task_distribution().await;
    demo_collective_decision().await;

    println!();
    println!("═══════════════════════════════════════════════════════════════════════");
    println!("                 SWARM INTELLIGENCE DEMO COMPLETE!");
    println!("═══════════════════════════════════════════════════════════════════════");
}

fn demo_agent_registry() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 1. AGENT REGISTRY & DISCOVERY                                         │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    let registry = AgentRegistry::new();

    // Register various specialized agents
    let agents = vec![
        ("DataMiner-1", vec![AgentCapability::ContentExtraction, AgentCapability::Navigation]),
        ("DataMiner-2", vec![AgentCapability::ContentExtraction, AgentCapability::Navigation]),
        ("Analyst-1", vec![AgentCapability::KnowledgeManagement, AgentCapability::ContinualLearning]),
        ("Analyst-2", vec![AgentCapability::KnowledgeManagement, AgentCapability::ContinualLearning]),
        ("Validator-1", vec![AgentCapability::CodeExecution, AgentCapability::ApiAccess]),
        ("Coordinator-1", vec![AgentCapability::SwarmParticipation, AgentCapability::AgentCommunication]),
    ];

    for (name, caps) in agents {
        let agent = create_agent_with_capabilities(name, caps);
        // Manually set trust for demo
        let mut profile = agent.profile().clone();
        profile.trust_level = TrustLevel::Trusted;
        registry.register(profile, Some(format!("127.0.0.1:{}", rand::random::<u16>() % 10000 + 5000)));
        println!("  Registered: {}", name);
    }

    println!();
    println!("Online agents: {}", registry.online_agents().len());
    
    // Discovery by capability
    let extractors = registry.find_by_capability(&AgentCapability::ContentExtraction);
    println!("Agents with ContentExtraction: {}", extractors.len());
    for agent in &extractors {
        println!("  • {} ({:?})", agent.profile.name, agent.endpoint);
    }

    // Discovery by trust level
    let trusted = registry.find_by_trust(TrustLevel::Trusted);
    println!("\nTrusted agents: {}", trusted.len());
    println!();
}

async fn demo_swarm_formation() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 2. SWARM FORMATION                                                    │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    // Create a leader agent system
    let leader_system = create_agent_system("SwarmLeader");
    let coordinator = leader_system.swarm_coordinator.clone();

    // Register some worker agents
    for i in 1..=5 {
        let worker = create_agent_with_capabilities(
            format!("Worker-{}", i),
            vec![
                AgentCapability::ContentExtraction,
                AgentCapability::SwarmParticipation,
            ],
        );
        let mut profile = worker.profile().clone();
        profile.trust_level = TrustLevel::Trusted;
        leader_system.runtime.register_agent(profile);
    }

    println!("Registered {} worker agents", leader_system.runtime.known_agents().len());

    // Define a complex task
    let task = SwarmTask {
        id: Uuid::new_v4(),
        description: "Analyze global market trends across 20 sectors".to_string(),
        goal: Box::new(Goal::Custom {
            description: "Market trend analysis".to_string(),
            parameters: serde_json::json!({
                "sectors": 20,
                "depth": "comprehensive",
                "timeframe": "last_quarter"
            }),
        }),
        min_members: 3,
        max_members: 10,
        required_capabilities: vec![
            AgentCapability::ContentExtraction,
            AgentCapability::SwarmParticipation,
        ],
        deadline: None,
    };

    println!();
    println!("Task: {}", task.description);
    println!("  Min members: {}", task.min_members);
    println!("  Max members: {}", task.max_members);

    // Form the swarm
    let swarm_id = coordinator.create_swarm(task, None).await;
    println!();
    println!("✓ Created swarm: {}", swarm_id);
    println!("  Status: {:?}", coordinator.swarm_status(swarm_id));

    // Simulate agents joining
    for agent in leader_system.runtime.known_agents().iter().take(4) {
        coordinator.agent_joined(swarm_id, agent.id, SwarmRole::Worker);
        println!("  + {} joined as Worker", agent.name);
    }

    println!();
    println!("Swarm status after joining: {:?}", coordinator.swarm_status(swarm_id));
    println!();
}

async fn demo_task_distribution() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 3. TASK DISTRIBUTION & EXECUTION                                      │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    let leader_system = create_agent_system("DistributionLeader");
    let coordinator = leader_system.swarm_coordinator.clone();

    // Quick setup: register workers and form swarm
    let worker_ids: Vec<_> = (1..=5).map(|i| {
        let worker = create_agent_with_capabilities(
            format!("DistWorker-{}", i),
            vec![AgentCapability::ContentExtraction, AgentCapability::SwarmParticipation],
        );
        let mut profile = worker.profile().clone();
        profile.trust_level = TrustLevel::Trusted;
        let id = profile.id;
        leader_system.runtime.register_agent(profile);
        id
    }).collect();

    let task = SwarmTask {
        id: Uuid::new_v4(),
        description: "Distributed data collection".to_string(),
        goal: Box::new(Goal::Custom {
            description: "Collect data from 100 sources".to_string(),
            parameters: serde_json::json!({ "sources": 100 }),
        }),
        min_members: 3,
        max_members: 10,
        required_capabilities: vec![AgentCapability::ContentExtraction],
        deadline: None,
    };

    let swarm_id = coordinator.create_swarm(task, None).await;
    for (i, worker_id) in worker_ids.iter().enumerate() {
        let role = if i == 0 { SwarmRole::Coordinator } else { SwarmRole::Worker };
        coordinator.agent_joined(swarm_id, *worker_id, role);
    }

    // Distribute tasks
    println!("Distributing tasks to swarm members...");
    let assignments = coordinator.distribute_tasks(swarm_id);
    
    for (agent_id, tasks) in &assignments {
        println!("  Agent {:?}: {} subtask(s)", agent_id.0.to_string().chars().take(8).collect::<String>(), tasks.len());
    }

    // Simulate workers submitting results
    println!();
    println!("Workers submitting results...");
    for (i, worker_id) in worker_ids.iter().enumerate() {
        let result = serde_json::json!({
            "worker": i,
            "data_collected": rand::random::<u32>() % 100 + 10,
            "quality_score": 0.8 + (rand::random::<f32>() * 0.2),
        });
        coordinator.submit_result(swarm_id, *worker_id, result.clone());
        println!("  Worker {} submitted: {} items", i, result["data_collected"]);
    }

    // Aggregate results
    println!();
    if let Some(final_result) = coordinator.aggregate_results(swarm_id) {
        println!("✓ Swarm completed!");
        println!("  Final result summary:");
        println!("    Members: {}", final_result["members"]);
        println!("    Partial results: {}", 
            final_result["partial_results"].as_array().map(|a| a.len()).unwrap_or(0));
    }
    println!();
}

async fn demo_collective_decision() {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 4. COLLECTIVE DECISION MAKING                                         │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    println!("Swarm Consensus Protocol:");
    println!();
    println!("  ┌─────────────────────────────────────────────────────────────────┐");
    println!("  │                    CONSENSUS MECHANISM                          │");
    println!("  ├─────────────────────────────────────────────────────────────────┤");
    println!("  │                                                                 │");
    println!("  │   ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐  │");
    println!("  │   │Agent 1 │  │Agent 2 │  │Agent 3 │  │Agent 4 │  │Agent 5 │  │");
    println!("  │   │  Vote  │  │  Vote  │  │  Vote  │  │  Vote  │  │  Vote  │  │");
    println!("  │   │   A    │  │   B    │  │   A    │  │   A    │  │   B    │  │");
    println!("  │   └───┬────┘  └───┬────┘  └───┬────┘  └───┬────┘  └───┬────┘  │");
    println!("  │       │          │          │          │          │          │");
    println!("  │       └──────────┴──────────┼──────────┴──────────┘          │");
    println!("  │                             │                                 │");
    println!("  │                             ▼                                 │");
    println!("  │                    ┌────────────────┐                        │");
    println!("  │                    │  AGGREGATION   │                        │");
    println!("  │                    │  A: 3 votes    │                        │");
    println!("  │                    │  B: 2 votes    │                        │");
    println!("  │                    └───────┬────────┘                        │");
    println!("  │                            │                                  │");
    println!("  │                            ▼                                  │");
    println!("  │                    ┌────────────────┐                        │");
    println!("  │                    │   CONSENSUS    │                        │");
    println!("  │                    │   REACHED: A   │                        │");
    println!("  │                    │   (60% > 67%?) │                        │");
    println!("  │                    └────────────────┘                        │");
    println!("  │                                                               │");
    println!("  └─────────────────────────────────────────────────────────────────┘");
    println!();

    // Simulate a voting scenario
    let votes = vec![
        ("Agent-1", "Option A", 0.9),
        ("Agent-2", "Option B", 0.7),
        ("Agent-3", "Option A", 0.85),
        ("Agent-4", "Option A", 0.95),
        ("Agent-5", "Option B", 0.6),
    ];

    println!("Weighted Voting Results:");
    let mut option_a_weight = 0.0;
    let mut option_b_weight = 0.0;
    
    for (agent, vote, confidence) in &votes {
        println!("  {} → {} (confidence: {:.0}%)", agent, vote, confidence * 100.0);
        if *vote == "Option A" {
            option_a_weight += confidence;
        } else {
            option_b_weight += confidence;
        }
    }
    
    let total_weight = option_a_weight + option_b_weight;
    let a_percentage = option_a_weight / total_weight;
    let b_percentage = option_b_weight / total_weight;
    
    println!();
    println!("Weighted Results:");
    println!("  Option A: {:.1}%", a_percentage * 100.0);
    println!("  Option B: {:.1}%", b_percentage * 100.0);
    
    let threshold = 0.67;
    if a_percentage >= threshold {
        println!();
        println!("✓ CONSENSUS REACHED: Option A (exceeds {:.0}% threshold)", threshold * 100.0);
    } else if b_percentage >= threshold {
        println!();
        println!("✓ CONSENSUS REACHED: Option B (exceeds {:.0}% threshold)", threshold * 100.0);
    } else {
        println!();
        println!("⚠ NO CONSENSUS: Neither option reached {:.0}% threshold", threshold * 100.0);
        println!("  → Initiating second round of deliberation...");
    }
    println!();
}
