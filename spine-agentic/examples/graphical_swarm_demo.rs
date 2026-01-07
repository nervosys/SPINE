//! Graphical Model Swarm Optimization Demo
//!
//! Demonstrates the use of different graphical model topologies for swarm coordination:
//! - DAG: Task dependency chains
//! - Bayesian Network: Probabilistic task assignment
//! - Markov Random Field: Pairwise agent coordination
//! - Factor Graph: Complex multi-agent constraints
//! - Hypergraph: Consensus requirements
//! - Dynamic Bayesian Network: Temporal task sequences
//! - Conditional Random Field: Structured prediction

use chrono::Utc;
use spine_agentic::{
    AgentCapability, AgentId, FactorPotential, Goal, GraphicalModelType, GraphicalSwarmOptimizer,
    HyperEdgeConstraint, InferenceResult, Swarm, SwarmGraphicalModel, SwarmMember, SwarmRole,
    SwarmStatus, SwarmTask,
};
use uuid::Uuid;

fn main() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║     HYPERLIGHT GRAPHICAL MODEL SWARM OPTIMIZATION DEMO          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // Demo all graphical model types
    demo_dag();
    demo_bayesian_network();
    demo_markov_random_field();
    demo_factor_graph();
    demo_hypergraph();
    demo_dynamic_bayesian();
    demo_conditional_random_field();

    // Benchmark comparison
    benchmark_all_models();

    println!("\n✅ All graphical model demonstrations complete!");
}

fn demo_dag() {
    println!("┌──────────────────────────────────────────────────────────────────┐");
    println!("│ 1. DIRECTED ACYCLIC GRAPH (DAG)                                 │");
    println!("│    Use case: Task dependency chains, causal inference           │");
    println!("└──────────────────────────────────────────────────────────────────┘");

    let mut model = SwarmGraphicalModel::new(GraphicalModelType::DAG);

    // Create agent nodes (task executors in sequence)
    let agent1 = model.add_agent_node(AgentId::new_v4(), vec![0.8, 0.2]); // High capability
    let agent2 = model.add_agent_node(AgentId::new_v4(), vec![0.6, 0.4]); // Medium capability
    let agent3 = model.add_agent_node(AgentId::new_v4(), vec![0.4, 0.6]); // Depends on others

    // Add directed edges (dependencies)
    model.add_directed_edge(
        agent1,
        agent2,
        Some(vec![
            vec![0.9, 0.1], // If agent1 succeeds, agent2 likely succeeds
            vec![0.3, 0.7], // If agent1 fails, agent2 may still succeed
        ]),
    );
    model.add_directed_edge(agent2, agent3, Some(vec![vec![0.95, 0.05], vec![0.2, 0.8]]));

    let result = model.run_belief_propagation();
    print_inference_result("DAG", &result);
}

fn demo_bayesian_network() {
    println!("\n┌──────────────────────────────────────────────────────────────────┐");
    println!("│ 2. BAYESIAN NETWORK                                             │");
    println!("│    Use case: Probabilistic task assignment with uncertainty     │");
    println!("└──────────────────────────────────────────────────────────────────┘");

    let mut model = SwarmGraphicalModel::new(GraphicalModelType::BayesianNetwork);

    // Skill nodes (latent variables)
    let skill_analysis = model.add_task_node(3); // Low/Medium/High
    let skill_coding = model.add_task_node(3);

    // Agent nodes (observed)
    let agent1 = model.add_agent_node(AgentId::new_v4(), vec![0.1, 0.3, 0.6]); // High skill
    let agent2 = model.add_agent_node(AgentId::new_v4(), vec![0.3, 0.5, 0.2]); // Medium skill

    // CPT: P(Agent | Skill)
    model.add_directed_edge(
        skill_analysis,
        agent1,
        Some(vec![
            vec![0.8, 0.15, 0.05], // Low skill -> low performance
            vec![0.2, 0.6, 0.2],   // Medium skill
            vec![0.05, 0.25, 0.7], // High skill -> high performance
        ]),
    );
    model.add_directed_edge(
        skill_coding,
        agent2,
        Some(vec![
            vec![0.7, 0.2, 0.1],
            vec![0.3, 0.5, 0.2],
            vec![0.1, 0.3, 0.6],
        ]),
    );

    let result = model.run_belief_propagation();
    print_inference_result("Bayesian Network", &result);
}

fn demo_markov_random_field() {
    println!("\n┌──────────────────────────────────────────────────────────────────┐");
    println!("│ 3. MARKOV RANDOM FIELD (MRF)                                    │");
    println!("│    Use case: Pairwise agent coordination, undirected relations  │");
    println!("└──────────────────────────────────────────────────────────────────┘");

    let mut model = SwarmGraphicalModel::new(GraphicalModelType::MarkovRandomField);

    // Agents that need to coordinate
    let agent1 = model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]);
    let agent2 = model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]);
    let agent3 = model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]);
    let agent4 = model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]);

    // Pairwise potentials (favor agreement)
    let agreement_potential = vec![
        vec![2.0, 0.5], // Both inactive: good
        vec![0.5, 2.0], // Both active: good
    ];

    // Grid topology connections
    model.add_undirected_edge(agent1, agent2, agreement_potential.clone());
    model.add_undirected_edge(agent2, agent3, agreement_potential.clone());
    model.add_undirected_edge(agent3, agent4, agreement_potential.clone());
    model.add_undirected_edge(agent4, agent1, agreement_potential.clone());
    model.add_undirected_edge(agent1, agent3, agreement_potential.clone()); // Diagonal

    let result = model.run_belief_propagation();
    print_inference_result("Markov Random Field", &result);
}

fn demo_factor_graph() {
    println!("\n┌──────────────────────────────────────────────────────────────────┐");
    println!("│ 4. FACTOR GRAPH                                                 │");
    println!("│    Use case: Complex multi-agent constraints, resource sharing  │");
    println!("└──────────────────────────────────────────────────────────────────┘");

    let mut model = SwarmGraphicalModel::new(GraphicalModelType::FactorGraph);

    // Agent variable nodes
    let agents: Vec<Uuid> = (0..5)
        .map(|_| model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]))
        .collect();

    // Resource constraint factor: at most 2 agents can use resource simultaneously
    let resource_constraint = {
        let mut table = vec![0.0; 32]; // 2^5 configurations
        for config in 0..32u32 {
            let active_count = config.count_ones();
            table[config as usize] = if active_count <= 2 { 1.0 } else { 0.01 };
        }
        table
    };

    model.add_factor(
        agents.clone(),
        FactorPotential::Table(resource_constraint),
        vec![2; 5], // Binary variables
    );

    // Quality factor: prefer diverse skills
    let quality_factor = vec![
        0.5, // 00000 - no agents: bad
        1.0, // 00001
        1.0, // 00010
        1.2, // 00011 - pair bonus
        1.0, // ... etc
        1.2, 1.2, 1.5, // Diversity bonuses
        1.0, 1.2, 1.2, 1.5, 1.0, 1.2, 1.2, 1.5, 1.0, 1.2, 1.2, 1.5, 1.2, 1.5, 1.5, 1.8, 1.0, 1.2,
        1.2, 1.5, 1.2, 1.5, 1.5, 0.8, // Too many: penalty
    ];
    model.add_factor(
        agents[0..4].to_vec(),
        FactorPotential::Table(quality_factor),
        vec![2; 4],
    );

    let result = model.run_belief_propagation();
    print_inference_result("Factor Graph", &result);
}

fn demo_hypergraph() {
    println!("\n┌──────────────────────────────────────────────────────────────────┐");
    println!("│ 5. HYPERGRAPH                                                   │");
    println!("│    Use case: Multi-way consensus, group coordination            │");
    println!("└──────────────────────────────────────────────────────────────────┘");

    let mut model = SwarmGraphicalModel::new(GraphicalModelType::Hypergraph);

    // Team of agents
    let team_a: Vec<Uuid> = (0..3)
        .map(|_| model.add_agent_node(AgentId::new_v4(), vec![0.6, 0.4]))
        .collect();
    let team_b: Vec<Uuid> = (0..3)
        .map(|_| model.add_agent_node(AgentId::new_v4(), vec![0.4, 0.6]))
        .collect();

    // Consensus hyperedge: Team A must agree
    model.add_hyperedge(team_a.clone(), HyperEdgeConstraint::Consensus);

    // At-least-one hyperedge: At least one from Team B must be active
    model.add_hyperedge(team_b.clone(), HyperEdgeConstraint::AtLeastOne);

    // Exactly-2 hyperedge: Exactly 2 agents from both teams
    let mut cross_team = team_a[0..2].to_vec();
    cross_team.extend_from_slice(&team_b[0..2]);
    model.add_hyperedge(cross_team, HyperEdgeConstraint::ExactlyK(2));

    let result = model.run_belief_propagation();
    print_inference_result("Hypergraph", &result);
}

fn demo_dynamic_bayesian() {
    println!("\n┌──────────────────────────────────────────────────────────────────┐");
    println!("│ 6. DYNAMIC BAYESIAN NETWORK                                     │");
    println!("│    Use case: Temporal task sequences, time-series coordination  │");
    println!("└──────────────────────────────────────────────────────────────────┘");

    let mut model = SwarmGraphicalModel::new(GraphicalModelType::DynamicBayesian);

    // Time slices: t=0, t=1, t=2
    let t0_agent = model.add_agent_node(AgentId::new_v4(), vec![0.8, 0.2]); // Initial state
    let t1_agent = model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]); // Influenced by t0
    let t2_agent = model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]); // Influenced by t1

    // Transition CPTs
    let transition_cpt = vec![
        vec![0.7, 0.3], // Stay inactive if was inactive
        vec![0.2, 0.8], // Stay active if was active
    ];

    model.add_directed_edge(t0_agent, t1_agent, Some(transition_cpt.clone()));
    model.add_directed_edge(t1_agent, t2_agent, Some(transition_cpt));

    let result = model.run_belief_propagation();
    print_inference_result("Dynamic Bayesian Network", &result);
}

fn demo_conditional_random_field() {
    println!("\n┌──────────────────────────────────────────────────────────────────┐");
    println!("│ 7. CONDITIONAL RANDOM FIELD (CRF)                               │");
    println!("│    Use case: Structured prediction, sequence labeling           │");
    println!("└──────────────────────────────────────────────────────────────────┘");

    let mut model = SwarmGraphicalModel::new(GraphicalModelType::ConditionalRandomField);

    // Observation nodes (task features)
    let obs1 = model.add_task_node(3); // Task complexity: Low/Medium/High
    let obs2 = model.add_task_node(3);
    let obs3 = model.add_task_node(3);

    // Label nodes (agent assignments)
    let agent1 = model.add_agent_node(AgentId::new_v4(), vec![0.33, 0.33, 0.34]);
    let agent2 = model.add_agent_node(AgentId::new_v4(), vec![0.33, 0.33, 0.34]);
    let agent3 = model.add_agent_node(AgentId::new_v4(), vec![0.33, 0.33, 0.34]);

    // Feature functions (observation -> label)
    let feature_potential = vec![
        vec![2.0, 1.0, 0.5], // Easy task -> junior agent
        vec![1.0, 2.0, 1.0], // Medium task -> mid-level agent
        vec![0.5, 1.0, 2.0], // Hard task -> senior agent
    ];

    model.add_undirected_edge(obs1, agent1, feature_potential.clone());
    model.add_undirected_edge(obs2, agent2, feature_potential.clone());
    model.add_undirected_edge(obs3, agent3, feature_potential.clone());

    // Transition features (label -> label)
    let transition_potential = vec![
        vec![1.5, 1.0, 0.5],
        vec![1.0, 1.5, 1.0],
        vec![0.5, 1.0, 1.5],
    ];
    model.add_undirected_edge(agent1, agent2, transition_potential.clone());
    model.add_undirected_edge(agent2, agent3, transition_potential);

    let result = model.run_belief_propagation();
    print_inference_result("Conditional Random Field", &result);
}

fn benchmark_all_models() {
    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║              GRAPHICAL MODEL BENCHMARK COMPARISON               ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    let model_types = [
        GraphicalModelType::DAG,
        GraphicalModelType::BayesianNetwork,
        GraphicalModelType::MarkovRandomField,
        GraphicalModelType::FactorGraph,
        GraphicalModelType::Hypergraph,
        GraphicalModelType::DynamicBayesian,
        GraphicalModelType::ConditionalRandomField,
    ];

    println!("┌──────────────────────┬────────────┬────────────┬────────────┐");
    println!("│ Model Type           │ Time (μs)  │ Iterations │ Converged  │");
    println!("├──────────────────────┼────────────┼────────────┼────────────┤");

    for model_type in &model_types {
        let (time_us, iterations, converged) = benchmark_model(*model_type);
        println!(
            "│ {:20} │ {:>10} │ {:>10} │ {:>10} │",
            format!("{:?}", model_type),
            time_us,
            iterations,
            if converged { "✓" } else { "✗" }
        );
    }

    println!("└──────────────────────┴────────────┴────────────┴────────────┘");

    // Create a sample swarm and run optimizer
    println!("\n📊 Swarm Optimization Demo:");
    let mut optimizer = GraphicalSwarmOptimizer::new(GraphicalModelType::FactorGraph);

    let swarm = create_sample_swarm(5);
    let model_id = optimizer.create_model_for_swarm(&swarm);

    println!("   Created model for swarm: {}", swarm.name);
    println!(
        "   Auto-selected model type: {:?}",
        optimizer
            .models
            .get(&model_id)
            .map(|m| m.model_type)
            .unwrap_or(GraphicalModelType::DAG)
    );

    if let Some(result) = optimizer.optimize_swarm(model_id) {
        println!(
            "   Optimization converged: {} in {} iterations",
            result.converged, result.iterations
        );
        println!("   Free energy: {:.4}", result.free_energy);
        println!("   Assignments: {} nodes", result.assignment.len());
    }
}

fn benchmark_model(model_type: GraphicalModelType) -> (u64, usize, bool) {
    let start = std::time::Instant::now();

    let mut model = SwarmGraphicalModel::new(model_type);

    // Create 10 agent nodes
    let agents: Vec<Uuid> = (0..10)
        .map(|_| model.add_agent_node(AgentId::new_v4(), vec![0.5, 0.5]))
        .collect();

    // Add appropriate structure based on model type
    match model_type {
        GraphicalModelType::DAG
        | GraphicalModelType::BayesianNetwork
        | GraphicalModelType::DynamicBayesian => {
            for i in 0..agents.len() - 1 {
                model.add_directed_edge(agents[i], agents[i + 1], None);
            }
        }
        GraphicalModelType::MarkovRandomField | GraphicalModelType::ConditionalRandomField => {
            for i in 0..agents.len() - 1 {
                model.add_undirected_edge(
                    agents[i],
                    agents[i + 1],
                    vec![vec![1.5, 0.5], vec![0.5, 1.5]],
                );
            }
        }
        GraphicalModelType::FactorGraph => {
            model.add_factor(
                agents.clone(),
                FactorPotential::Table(vec![1.0; 1024]),
                vec![2; 10],
            );
        }
        GraphicalModelType::Hypergraph => {
            model.add_hyperedge(agents[0..5].to_vec(), HyperEdgeConstraint::Consensus);
            model.add_hyperedge(agents[5..10].to_vec(), HyperEdgeConstraint::AtLeastOne);
        }
    }

    let result = model.run_belief_propagation();
    let elapsed = start.elapsed();

    (
        elapsed.as_micros() as u64,
        result.iterations,
        result.converged,
    )
}

fn create_sample_swarm(num_members: usize) -> Swarm {
    let members: Vec<SwarmMember> = (0..num_members)
        .map(|i| SwarmMember {
            agent_id: AgentId::new_v4(),
            role: if i == 0 {
                SwarmRole::Leader
            } else {
                SwarmRole::Worker
            },
            joined_at: Utc::now(),
            contribution_score: 0.0,
        })
        .collect();

    Swarm {
        id: Uuid::new_v4(),
        name: format!("BenchmarkSwarm-{}", num_members),
        task: SwarmTask {
            id: Uuid::new_v4(),
            description: "Benchmark task".to_string(),
            goal: Box::new(Goal::Custom {
                description: "Benchmark swarm optimization".to_string(),
                parameters: serde_json::json!({}),
            }),
            required_capabilities: vec![
                AgentCapability::CodeExecution,
                AgentCapability::ContentExtraction,
            ],
            min_members: 2,
            max_members: 10,
            deadline: None,
        },
        members,
        leader: None,
        created_at: Utc::now(),
        status: SwarmStatus::Active,
        consensus_threshold: 0.67,
    }
}

fn print_inference_result(name: &str, result: &InferenceResult) {
    println!("   📈 {} Inference Results:", name);
    println!("      • Converged: {}", result.converged);
    println!("      • Iterations: {}", result.iterations);
    println!(
        "      • Partition function: {:.4}",
        result.partition_function
    );
    println!(
        "      • Marginals computed: {} nodes",
        result.marginals.len()
    );

    // Show sample marginals
    for (i, (node_id, marginal)) in result.marginals.iter().take(3).enumerate() {
        let formatted: String = marginal
            .iter()
            .map(|p| format!("{:.3}", p))
            .collect::<Vec<_>>()
            .join(", ");
        println!("      • Node {}: [{}]", i + 1, formatted);
    }
    if result.marginals.len() > 3 {
        println!("      • ... and {} more nodes", result.marginals.len() - 3);
    }
}
