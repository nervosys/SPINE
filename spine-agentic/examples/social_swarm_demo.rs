//! Social Network Multi-Agent Swarm Demo
//!
//! Demonstrates collaborative agent roles and social network structures
//! as graphical models for multi-agent coordination.

use spine_agentic::{
    CollaborativeRole, SocialSwarmBuilder, SocialSwarmNetwork, SocialTopology,
};
use std::collections::HashMap;

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║      SOCIAL NETWORK MULTI-AGENT SWARM DEMO                  ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Demo 1: Hierarchical Organization
    demo_hierarchical_swarm();

    // Demo 2: Small-World Network (research collaboration)
    demo_small_world_swarm();

    // Demo 3: Modular Organization (departments)
    demo_modular_swarm();

    // Demo 4: Scale-Free Network (influencer network)
    demo_scale_free_swarm();

    // Demo 5: Dynamic Network Evolution
    demo_dynamic_evolution();

    println!("\n✅ All social swarm demos completed successfully!");
}

fn demo_hierarchical_swarm() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 1: Hierarchical Organization (Corporate Structure)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let network = SocialSwarmBuilder::new("Corporate Swarm")
        .topology(SocialTopology::Hierarchical {
            depth: 3,
            branching: 2,
        })
        // CEO
        .add_agent("CEO Alice", vec![CollaborativeRole::Coordinator])
        // VPs
        .add_agent(
            "VP Bob",
            vec![
                CollaborativeRole::Coordinator,
                CollaborativeRole::Aggregator,
            ],
        )
        .add_agent(
            "VP Carol",
            vec![CollaborativeRole::Coordinator, CollaborativeRole::Validator],
        )
        // Managers
        .add_agent(
            "Manager Dave",
            vec![CollaborativeRole::Expert {
                domain: "Engineering".to_string(),
            }],
        )
        .add_agent(
            "Manager Eve",
            vec![CollaborativeRole::Expert {
                domain: "Design".to_string(),
            }],
        )
        .add_agent(
            "Manager Frank",
            vec![CollaborativeRole::Expert {
                domain: "Marketing".to_string(),
            }],
        )
        .add_agent(
            "Manager Grace",
            vec![CollaborativeRole::Expert {
                domain: "Sales".to_string(),
            }],
        )
        // Workers
        .add_agent("Worker H1", vec![CollaborativeRole::Executor])
        .add_agent("Worker H2", vec![CollaborativeRole::Executor])
        .add_agent("Worker H3", vec![CollaborativeRole::Executor])
        .add_agent("Worker H4", vec![CollaborativeRole::Executor])
        .build();

    print_network_stats(&network);
    print_influence_ranking(&network, 5);

    // Distribute a task
    let task = network.distribute_task(
        "Launch new product line",
        &[
            CollaborativeRole::Expert {
                domain: "Engineering".to_string(),
            },
            CollaborativeRole::Expert {
                domain: "Marketing".to_string(),
            },
            CollaborativeRole::Validator,
        ],
    );
    print_task_distribution(&task);
}

fn demo_small_world_swarm() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 2: Small-World Network (Research Collaboration)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mut expertise = HashMap::new();
    expertise.insert("ML".to_string(), 0.9);
    expertise.insert("NLP".to_string(), 0.8);

    let network = SocialSwarmBuilder::new("Research Lab")
        .topology(SocialTopology::SmallWorld { rewire_prob: 0.3 })
        .add_agent_with_expertise(
            "Dr. Smith (ML Lead)",
            vec![
                CollaborativeRole::Expert {
                    domain: "Machine Learning".to_string(),
                },
                CollaborativeRole::Coordinator,
            ],
            {
                let mut e = HashMap::new();
                e.insert("ML".to_string(), 0.95);
                e.insert("Statistics".to_string(), 0.8);
                e
            },
        )
        .add_agent_with_expertise(
            "Dr. Johnson (NLP)",
            vec![
                CollaborativeRole::Expert {
                    domain: "NLP".to_string(),
                },
                CollaborativeRole::Innovator,
            ],
            {
                let mut e = HashMap::new();
                e.insert("NLP".to_string(), 0.9);
                e.insert("Linguistics".to_string(), 0.85);
                e
            },
        )
        .add_agent_with_expertise(
            "Dr. Williams (CV)",
            vec![CollaborativeRole::Expert {
                domain: "Computer Vision".to_string(),
            }],
            {
                let mut e = HashMap::new();
                e.insert("CV".to_string(), 0.9);
                e.insert("Graphics".to_string(), 0.7);
                e
            },
        )
        .add_agent(
            "PhD Student Alice",
            vec![CollaborativeRole::Executor, CollaborativeRole::Learner],
        )
        .add_agent(
            "PhD Student Bob",
            vec![CollaborativeRole::Executor, CollaborativeRole::Learner],
        )
        .add_agent(
            "PhD Student Carol",
            vec![CollaborativeRole::Executor, CollaborativeRole::Learner],
        )
        .add_agent(
            "Postdoc Dave",
            vec![CollaborativeRole::ProblemSolver, CollaborativeRole::Critic],
        )
        .add_agent(
            "Lab Manager Eve",
            vec![CollaborativeRole::Aggregator, CollaborativeRole::Archivist],
        )
        .build();

    print_network_stats(&network);
    print_influence_ranking(&network, 5);

    // Distribute a research task
    let task = network.distribute_task(
        "Develop multimodal AI model combining vision and language",
        &[
            CollaborativeRole::Expert {
                domain: "Computer Vision".to_string(),
            },
            CollaborativeRole::Expert {
                domain: "NLP".to_string(),
            },
            CollaborativeRole::ProblemSolver,
        ],
    );
    print_task_distribution(&task);
}

fn demo_modular_swarm() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 3: Modular Organization (Cross-Functional Teams)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let network = SocialSwarmBuilder::new("Product Teams")
        .topology(SocialTopology::Modular {
            num_clusters: 3,
            inter_cluster_prob: 0.1,
        })
        // Platform Team
        .add_agent(
            "Platform Lead",
            vec![
                CollaborativeRole::Coordinator,
                CollaborativeRole::Expert {
                    domain: "Infrastructure".to_string(),
                },
            ],
        )
        .add_agent("Backend Dev 1", vec![CollaborativeRole::Executor])
        .add_agent("Backend Dev 2", vec![CollaborativeRole::Executor])
        .add_agent(
            "DevOps Engineer",
            vec![CollaborativeRole::Expert {
                domain: "DevOps".to_string(),
            }],
        )
        // Frontend Team
        .add_agent(
            "Frontend Lead",
            vec![
                CollaborativeRole::Coordinator,
                CollaborativeRole::Expert {
                    domain: "UI/UX".to_string(),
                },
            ],
        )
        .add_agent("Frontend Dev 1", vec![CollaborativeRole::Executor])
        .add_agent("Frontend Dev 2", vec![CollaborativeRole::Executor])
        .add_agent("UX Designer", vec![CollaborativeRole::Innovator])
        // QA Team
        .add_agent(
            "QA Lead",
            vec![CollaborativeRole::Coordinator, CollaborativeRole::Validator],
        )
        .add_agent("QA Engineer 1", vec![CollaborativeRole::Validator])
        .add_agent("QA Engineer 2", vec![CollaborativeRole::Validator])
        .add_agent(
            "Automation Engineer",
            vec![
                CollaborativeRole::Executor,
                CollaborativeRole::Expert {
                    domain: "Testing".to_string(),
                },
            ],
        )
        .build();

    print_network_stats(&network);
    print_influence_ranking(&network, 6);

    // Distribute a cross-team task
    let task = network.distribute_task(
        "Implement new user authentication flow",
        &[
            CollaborativeRole::Expert {
                domain: "Infrastructure".to_string(),
            },
            CollaborativeRole::Expert {
                domain: "UI/UX".to_string(),
            },
            CollaborativeRole::Validator,
        ],
    );
    print_task_distribution(&task);
}

fn demo_scale_free_swarm() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 4: Scale-Free Network (Influencer Network)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let network = SocialSwarmBuilder::new("Influencer Network")
        .topology(SocialTopology::ScaleFree { initial_nodes: 3 })
        // Core influencers (hub nodes)
        .add_agent(
            "Mega Influencer",
            vec![CollaborativeRole::Coordinator, CollaborativeRole::Liaison],
        )
        .add_agent(
            "Industry Expert",
            vec![
                CollaborativeRole::Expert {
                    domain: "Tech".to_string(),
                },
                CollaborativeRole::Critic,
            ],
        )
        .add_agent(
            "Community Leader",
            vec![CollaborativeRole::Mediator, CollaborativeRole::Aggregator],
        )
        // Mid-tier
        .add_agent("Content Creator 1", vec![CollaborativeRole::Innovator])
        .add_agent("Content Creator 2", vec![CollaborativeRole::Innovator])
        .add_agent("Podcast Host", vec![CollaborativeRole::Liaison])
        .add_agent("Newsletter Author", vec![CollaborativeRole::Archivist])
        // Long tail
        .add_agent("Micro Influencer 1", vec![CollaborativeRole::Executor])
        .add_agent("Micro Influencer 2", vec![CollaborativeRole::Executor])
        .add_agent("Micro Influencer 3", vec![CollaborativeRole::Executor])
        .add_agent("Micro Influencer 4", vec![CollaborativeRole::Executor])
        .add_agent("Micro Influencer 5", vec![CollaborativeRole::Executor])
        .build();

    print_network_stats(&network);

    // Show influence distribution (should follow power law)
    println!("📊 Influence Distribution (Power Law expected):");
    let influential = network.get_influential_agents(12);
    for (i, (agent_id, score)) in influential.iter().enumerate() {
        if let Some(agent) = network.agents.get(agent_id) {
            let bar_len = (score * 50.0) as usize;
            println!(
                "  {:2}. {:20} [{:>5.1}%] {}",
                i + 1,
                agent.name,
                score * 100.0,
                "█".repeat(bar_len.max(1))
            );
        }
    }
}

fn demo_dynamic_evolution() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 5: Dynamic Network Evolution");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mut network = SocialSwarmBuilder::new("Evolving Team")
        .topology(SocialTopology::Dynamic)
        .add_agent("Agent Alpha", vec![CollaborativeRole::Coordinator])
        .add_agent("Agent Beta", vec![CollaborativeRole::Executor])
        .add_agent("Agent Gamma", vec![CollaborativeRole::Executor])
        .add_agent("Agent Delta", vec![CollaborativeRole::Innovator])
        .add_agent("Agent Epsilon", vec![CollaborativeRole::Validator])
        .add_agent("Agent Zeta", vec![CollaborativeRole::ProblemSolver])
        .build();

    println!("📈 Initial Network State:");
    print_network_stats(&network);

    // Collect agent IDs for interactions
    let agent_ids: Vec<_> = network.agents.iter().map(|a| *a.key()).collect();

    // Simulate successful interactions (e.g., Alpha coordinates well with Beta, Gamma)
    let successful = vec![
        (agent_ids[0], agent_ids[1]), // Alpha -> Beta
        (agent_ids[0], agent_ids[2]), // Alpha -> Gamma
        (agent_ids[1], agent_ids[2]), // Beta -> Gamma (team collaboration)
        (agent_ids[3], agent_ids[4]), // Delta -> Epsilon (innovation validated)
    ];

    // Some failed interactions
    let failed = vec![
        (agent_ids[4], agent_ids[5]), // Epsilon and Zeta conflict
    ];

    println!("\n🔄 Simulating interactions...");
    println!("   ✓ {} successful interactions", successful.len());
    println!("   ✗ {} failed interactions", failed.len());

    network.evolve(&successful, &failed);

    println!("\n📈 After Evolution:");
    print_network_stats(&network);

    // Show how relationships changed
    println!("\n🔗 Strong Relationships (strength > 0.8):");
    for rel in &network.relationships {
        if rel.strength > 0.8 {
            if let (Some(from), Some(to)) =
                (network.agents.get(&rel.from), network.agents.get(&rel.to))
            {
                println!(
                    "   {} → {} (strength: {:.2}, trust: {:.2})",
                    from.name, to.name, rel.strength, rel.trust
                );
            }
        }
    }

    // Simulate more evolution rounds
    for round in 1..=3 {
        // More successful collaborations form based on role complementarity
        let new_successful: Vec<_> = if round % 2 == 0 {
            vec![(agent_ids[1], agent_ids[3]), (agent_ids[2], agent_ids[5])]
        } else {
            vec![(agent_ids[0], agent_ids[4]), (agent_ids[3], agent_ids[5])]
        };

        network.evolve(&new_successful, &[]);
    }

    println!("\n📈 After 3 More Evolution Rounds:");
    print_network_stats(&network);
    print_influence_ranking(&network, 6);
}

fn print_network_stats(network: &SocialSwarmNetwork) {
    let stats = network.stats();
    println!("📊 Network Statistics:");
    println!("   • Agents: {}", stats.num_agents);
    println!("   • Relationships: {}", stats.num_relationships);
    println!("   • Density: {:.2}%", stats.density * 100.0);
    println!("   • Avg Trust: {:.2}", stats.avg_trust);
    println!("   • Avg Strength: {:.2}", stats.avg_strength);
    println!("   • Topology: {}", stats.topology);
}

fn print_influence_ranking(network: &SocialSwarmNetwork, limit: usize) {
    println!("\n🏆 Top {} Most Influential Agents:", limit);
    let influential = network.get_influential_agents(limit);
    for (i, (agent_id, score)) in influential.iter().enumerate() {
        if let Some(agent) = network.agents.get(agent_id) {
            let roles: Vec<String> = agent.roles.iter().map(|r| format!("{:?}", r)).collect();
            println!(
                "   {:2}. {} (influence: {:.3}) - roles: [{}]",
                i + 1,
                agent.name,
                score,
                roles.join(", ")
            );
        }
    }
}

fn print_task_distribution(task: &spine_agentic::TaskDistribution) {
    println!("\n📋 Task Distribution: \"{}\"", task.description);
    println!("   Task ID: {}", task.task_id);
    if let Some(coord) = task.coordinator {
        println!("   Coordinator: {:?}", coord);
    }
    println!("   Assignments:");
    for assignment in &task.assignments {
        println!(
            "      • {:?} -> {} (priority: {:.1})",
            assignment.role, assignment.subtask, assignment.priority
        );
        if !assignment.dependencies.is_empty() {
            println!("        Dependencies: {:?}", assignment.dependencies);
        }
    }
}
