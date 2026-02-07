//! # Autonomous Agent Demo
//!
//! Demonstrates an autonomous agent using behavior trees to perform complex
//! multi-step tasks without human intervention.
//!
//! Run: cargo run --example autonomous_agent -p spine-agentic

use spine_agentic::{
    create_agent_system, Action, Condition, Goal, ResourceLocator,
    BehaviorNode, BehaviorResult, AgentCapability, SemanticQuery, OutputType,
};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    println!("╔═══════════════════════════════════════════════════════════════════════╗");
    println!("║              AUTONOMOUS AGENT DEMONSTRATION                           ║");
    println!("║        Behavior Trees for Self-Directed Agent Operation              ║");
    println!("╚═══════════════════════════════════════════════════════════════════════╝");
    println!();

    // Create a full agent system
    let system = create_agent_system("AutonomousResearcher");
    println!("✓ Created agent system: {}", system.runtime.profile().name);
    println!("  ID: {:?}", system.runtime.agent_id());
    println!();

    demo_simple_behavior(&system).await;
    demo_complex_behavior(&system).await;
    demo_goal_achievement(&system).await;

    println!();
    println!("═══════════════════════════════════════════════════════════════════════");
    println!("                  AUTONOMOUS AGENT DEMO COMPLETE!");
    println!("═══════════════════════════════════════════════════════════════════════");
}

async fn demo_simple_behavior(system: &spine_agentic::AgentSystem) {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 1. SIMPLE BEHAVIOR TREE                                               │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    // Simple sequence: Navigate -> Extract -> Store
    let behavior = BehaviorNode::Sequence(vec![
        BehaviorNode::Action(Action::Navigate(
            ResourceLocator::semantic("AI research papers")
                .with_constraint("topic:transformers")
        )),
        BehaviorNode::Action(Action::Extract(SemanticQuery {
            query: "Key findings and contributions".to_string(),
            output_type: OutputType::List,
            context: vec!["academic".to_string()],
            confidence_threshold: 0.8,
        })),
        BehaviorNode::Action(Action::Store {
            key: "research_findings".to_string(),
            value: serde_json::json!({ "source": "transformers_papers" }),
        }),
    ]);

    println!("Behavior Tree:");
    println!("  Sequence");
    println!("    ├── Navigate → AI research papers");
    println!("    ├── Extract → Key findings");
    println!("    └── Store → research_findings");
    println!();

    let result = system.run_behavior(behavior).await;
    println!("Execution result: {:?}", result);
    println!();
}

async fn demo_complex_behavior(system: &spine_agentic::AgentSystem) {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 2. COMPLEX BEHAVIOR TREE WITH BRANCHING                               │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    // Complex behavior with conditionals and parallel execution
    let behavior = BehaviorNode::Sequence(vec![
        // First, check if we have the capability
        BehaviorNode::Condition(Condition::HasCapability(AgentCapability::Navigation)),
        
        // Then try to get data from multiple sources in parallel
        BehaviorNode::Parallel {
            children: vec![
                BehaviorNode::Action(Action::Navigate(ResourceLocator::url("https://arxiv.org"))),
                BehaviorNode::Action(Action::Navigate(ResourceLocator::url("https://scholar.google.com"))),
                BehaviorNode::Action(Action::Navigate(ResourceLocator::url("https://semantic-scholar.org"))),
            ],
            success_threshold: 2, // Need at least 2 to succeed
        },
        
        // Select best approach based on conditions
        BehaviorNode::Selector(vec![
            // Try the fast path first
            BehaviorNode::Sequence(vec![
                BehaviorNode::Condition(Condition::ResourceExists(
                    ResourceLocator::semantic("cached_results")
                )),
                BehaviorNode::Action(Action::Retrieve { key: "cached_results".to_string() }),
            ]),
            // Fall back to full extraction
            BehaviorNode::Action(Action::Extract(SemanticQuery {
                query: "Extract all relevant information".to_string(),
                output_type: OutputType::Json(None),
                context: vec![],
                confidence_threshold: 0.7,
            })),
        ]),
        
        // Always learn from the experience
        BehaviorNode::Succeeder(Box::new(
            BehaviorNode::Action(Action::Learn { topic: "research_patterns".to_string() })
        )),
    ]);

    println!("Behavior Tree:");
    println!("  Sequence");
    println!("    ├── Condition: HasCapability(Navigation)");
    println!("    ├── Parallel (need 2/3)");
    println!("    │     ├── Navigate → arxiv.org");
    println!("    │     ├── Navigate → scholar.google.com");
    println!("    │     └── Navigate → semantic-scholar.org");
    println!("    ├── Selector (try in order)");
    println!("    │     ├── [Cache] Check & Retrieve");
    println!("    │     └── [Fallback] Full Extraction");
    println!("    └── Succeeder(Learn)");
    println!();

    let result = system.run_behavior(behavior).await;
    println!("Execution result: {:?}", result);
    println!();
}

async fn demo_goal_achievement(system: &spine_agentic::AgentSystem) {
    println!("┌───────────────────────────────────────────────────────────────────────┐");
    println!("│ 3. END-TO-END GOAL ACHIEVEMENT                                        │");
    println!("└───────────────────────────────────────────────────────────────────────┘");
    println!();

    // Define a high-level goal
    let goal = Goal::Extract {
        query: SemanticQuery {
            query: "Latest advances in quantum computing for machine learning".to_string(),
            output_type: OutputType::List,
            context: vec!["research".to_string(), "recent".to_string()],
            confidence_threshold: 0.85,
        },
        from: ResourceLocator::semantic("academic databases")
            .with_constraint("topic:quantum-ml")
            .with_constraint("year:2024"),
    };

    println!("Goal: Extract quantum ML advances from academic databases");
    println!();

    match system.achieve(goal).await {
        Ok(result) => {
            println!("✓ Goal achieved!");
            println!("  Result: {}", serde_json::to_string_pretty(&result).unwrap_or_default());
        }
        Err(e) => {
            println!("✗ Goal failed: {}", e);
        }
    }
    println!();

    // Show execution stats
    println!("Active intentions: {}", system.runtime.active_intentions().len());
    println!("Known agents: {}", system.runtime.known_agents().len());
}
