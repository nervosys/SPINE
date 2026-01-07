//! Adversarial Multi-Agent Game Theory Demo
//!
//! Demonstrates Nash equilibrium finding, minimax solving,
//! and regret-matching for competitive multi-agent scenarios.

use spine_agentic::{
    message_types, AdversarialArena, AgentId, CompactMessage, GameType, LightweightSwarm,
    MessagePool, MinimaxSolver, NashEquilibriumSolver, PayoffMatrix,
};

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     ADVERSARIAL MULTI-AGENT GAME THEORY DEMO                ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Demo 1: Prisoner's Dilemma
    demo_prisoners_dilemma();

    // Demo 2: Rock-Paper-Scissors
    demo_rock_paper_scissors();

    // Demo 3: Coordination Game
    demo_coordination_game();

    // Demo 4: Lightweight Swarm Communication
    demo_lightweight_swarm();

    // Demo 5: Message Pool Efficiency
    demo_message_pool();

    println!("\n✅ All adversarial demos completed successfully!");
}

fn demo_prisoners_dilemma() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 1: Prisoner's Dilemma - Nash Equilibrium");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Classic Prisoner's Dilemma payoff matrix
    // Actions: 0 = Cooperate, 1 = Defect
    // Payoffs: (Player1, Player2)
    //                    P2: Cooperate    P2: Defect
    // P1: Cooperate      (-1, -1)         (-3, 0)
    // P1: Defect         (0, -3)          (-2, -2)

    let players = vec![AgentId::new(), AgentId::new()];
    let actions = vec![
        vec!["Cooperate".to_string(), "Defect".to_string()],
        vec!["Cooperate".to_string(), "Defect".to_string()],
    ];

    let mut matrix = PayoffMatrix::new(players.clone(), actions);

    // Set payoffs
    matrix.set_payoff(&[0, 0], &[-1.0, -1.0]); // Both cooperate
    matrix.set_payoff(&[0, 1], &[-3.0, 0.0]); // P1 cooperates, P2 defects
    matrix.set_payoff(&[1, 0], &[0.0, -3.0]); // P1 defects, P2 cooperates
    matrix.set_payoff(&[1, 1], &[-2.0, -2.0]); // Both defect

    println!("Payoff Matrix:");
    println!("                  P2: Cooperate    P2: Defect");
    println!("  P1: Cooperate   (-1, -1)         (-3, 0)");
    println!("  P1: Defect      (0, -3)          (-2, -2)");

    // Find Nash equilibria
    let solver = NashEquilibriumSolver::new(GameType::MixedMotive);

    let pure_nash = solver.find_pure_nash(&matrix);
    println!("\n📊 Pure Strategy Nash Equilibria:");
    for eq in &pure_nash {
        let actions_str: Vec<&str> = eq
            .iter()
            .map(|&a| if a == 0 { "Cooperate" } else { "Defect" })
            .collect();
        println!("   {:?}", actions_str);
    }

    let mixed_nash = solver.find_mixed_nash(&matrix);
    println!("\n📊 Mixed Strategy Nash Equilibrium:");
    for (i, strategy) in mixed_nash.iter().enumerate() {
        println!(
            "   Player {}: Cooperate={:.1}%, Defect={:.1}%",
            i + 1,
            strategy[0] * 100.0,
            strategy[1] * 100.0
        );
    }

    // Run adversarial arena
    println!("\n🎮 Running 1000 rounds of iterated Prisoner's Dilemma...");
    let mut arena = AdversarialArena::new(GameType::MixedMotive, matrix);
    let stats = arena.run(1000);

    println!("\n📈 Arena Results:");
    println!("   Rounds played: {}", stats.rounds_played);
    for (i, &avg) in stats.avg_payoffs.iter().enumerate() {
        println!("   Player {} average payoff: {:.3}", i + 1, avg);
    }
    println!("   Nash distance: {:.4}", stats.nash_distance);
    println!(
        "   Converged to Nash: {}",
        if stats.converged { "Yes" } else { "No" }
    );
}

fn demo_rock_paper_scissors() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 2: Rock-Paper-Scissors - Zero-Sum Game");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let players = vec![AgentId::new(), AgentId::new()];
    let actions = vec![
        vec![
            "Rock".to_string(),
            "Paper".to_string(),
            "Scissors".to_string(),
        ],
        vec![
            "Rock".to_string(),
            "Paper".to_string(),
            "Scissors".to_string(),
        ],
    ];

    let mut matrix = PayoffMatrix::new(players, actions);

    // Zero-sum payoffs
    // R vs R = 0, R vs P = -1, R vs S = 1
    // P vs R = 1, P vs P = 0, P vs S = -1
    // S vs R = -1, S vs P = 1, S vs S = 0
    matrix.set_payoff(&[0, 0], &[0.0, 0.0]); // R vs R
    matrix.set_payoff(&[0, 1], &[-1.0, 1.0]); // R vs P
    matrix.set_payoff(&[0, 2], &[1.0, -1.0]); // R vs S
    matrix.set_payoff(&[1, 0], &[1.0, -1.0]); // P vs R
    matrix.set_payoff(&[1, 1], &[0.0, 0.0]); // P vs P
    matrix.set_payoff(&[1, 2], &[-1.0, 1.0]); // P vs S
    matrix.set_payoff(&[2, 0], &[-1.0, 1.0]); // S vs R
    matrix.set_payoff(&[2, 1], &[1.0, -1.0]); // S vs P
    matrix.set_payoff(&[2, 2], &[0.0, 0.0]); // S vs S

    // Minimax solution
    let minimax = MinimaxSolver::new(1);
    let (best_action, value) = minimax.solve(&matrix);
    println!("🎯 Minimax Solution:");
    println!(
        "   Best action for P1: {} (value: {:.2})",
        ["Rock", "Paper", "Scissors"][best_action],
        value
    );

    // Nash equilibrium (should be uniform 1/3 each)
    let solver = NashEquilibriumSolver::new(GameType::ZeroSum);
    let mixed_nash = solver.find_mixed_nash(&matrix);
    println!("\n📊 Mixed Strategy Nash Equilibrium:");
    for (i, strategy) in mixed_nash.iter().enumerate() {
        println!(
            "   Player {}: R={:.1}%, P={:.1}%, S={:.1}%",
            i + 1,
            strategy[0] * 100.0,
            strategy[1] * 100.0,
            strategy[2] * 100.0
        );
    }

    // Run arena
    println!("\n🎮 Running 5000 rounds...");
    let mut arena = AdversarialArena::new(GameType::ZeroSum, matrix);
    let stats = arena.run(5000);

    println!("\n📈 Arena Results:");
    println!("   Rounds played: {}", stats.rounds_played);
    for (i, &avg) in stats.avg_payoffs.iter().enumerate() {
        println!(
            "   Player {} average payoff: {:.4} (should be ~0)",
            i + 1,
            avg
        );
    }
    println!(
        "   Nash distance: {:.4} (should be <0.1)",
        stats.nash_distance
    );
    println!("   Converged: {}", if stats.converged { "✓" } else { "✗" });
}

fn demo_coordination_game() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 3: Coordination Game (Cooperative Equilibria)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Two agents choosing a meeting place
    // Both benefit from coordination

    let players = vec![AgentId::new(), AgentId::new()];
    let actions = vec![
        vec!["Location A".to_string(), "Location B".to_string()],
        vec!["Location A".to_string(), "Location B".to_string()],
    ];

    let mut matrix = PayoffMatrix::new(players, actions);

    // Both at A: (5, 5), Both at B: (3, 3), Mismatch: (0, 0)
    matrix.set_payoff(&[0, 0], &[5.0, 5.0]); // Both A
    matrix.set_payoff(&[0, 1], &[0.0, 0.0]); // A, B
    matrix.set_payoff(&[1, 0], &[0.0, 0.0]); // B, A
    matrix.set_payoff(&[1, 1], &[3.0, 3.0]); // Both B

    println!("Payoff Matrix (Coordination Game):");
    println!("                  P2: Location A   P2: Location B");
    println!("  P1: Location A  (5, 5)           (0, 0)");
    println!("  P1: Location B  (0, 0)           (3, 3)");

    let solver = NashEquilibriumSolver::new(GameType::Cooperative);

    let pure_nash = solver.find_pure_nash(&matrix);
    println!("\n📊 Pure Strategy Nash Equilibria:");
    for eq in &pure_nash {
        let actions_str: Vec<&str> = eq
            .iter()
            .map(|&a| if a == 0 { "Location A" } else { "Location B" })
            .collect();
        let payoff = matrix.get_payoff(eq, 0);
        println!(
            "   {:?} -> Payoff: ({:.0}, {:.0})",
            actions_str, payoff, payoff
        );
    }

    println!("\n🎮 Running 500 rounds with learning...");
    let mut arena = AdversarialArena::new(GameType::Cooperative, matrix);
    let stats = arena.run(500);

    println!("\n📈 Arena Results:");
    for (i, &avg) in stats.avg_payoffs.iter().enumerate() {
        println!("   Player {} average payoff: {:.2}", i + 1, avg);
    }

    // Show final strategies
    println!("\n📊 Learned Strategies:");
    for (i, agent) in arena.agents.iter().enumerate() {
        println!(
            "   Player {}: A={:.1}%, B={:.1}%",
            i + 1,
            agent.avg_strategy[0] * 100.0,
            agent.avg_strategy[1] * 100.0
        );
    }
}

fn demo_lightweight_swarm() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 4: Lightweight Swarm Communication");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mut swarm = LightweightSwarm::new(1);

    // Add agents
    for i in 1..=10 {
        swarm.add_agent(i);
    }

    println!("📡 Swarm created with {} agents", swarm.agents.len());
    println!("   Leader: Agent {}", swarm.leader.unwrap_or(0));

    // Send messages
    let task_payload = b"Execute task XYZ";
    let seq = swarm.send(1, 5, message_types::TASK_ASSIGN, task_payload);
    println!("\n📨 Sent task assignment (seq={})", seq);

    // Broadcast
    let broadcast_seqs = swarm.broadcast(1, message_types::BROADCAST, b"Hello swarm!");
    println!("📢 Broadcast {} messages", broadcast_seqs.len());

    // Heartbeats
    for agent in 2..=10 {
        swarm.send(agent, 1, message_types::HEARTBEAT, &[]);
    }
    println!("💓 {} heartbeats sent", 9);

    // Stats
    let stats = swarm.stats();
    println!("\n📊 Swarm Statistics:");
    println!("   ID: {}", stats.id);
    println!("   Agents: {}", stats.num_agents);
    println!("   Leader: {:?}", stats.leader);
    println!("   Pending messages: {}", stats.pending_messages);
    println!("   Total messages: {}", stats.total_messages);

    // Test compact message serialization
    let msg = CompactMessage::new(
        message_types::REQUEST,
        1,
        2,
        b"GET /resource HTTP/1.1".to_vec(),
    );
    let bytes = msg.to_bytes();
    let decoded = CompactMessage::from_bytes(&bytes).unwrap();

    println!("\n📦 Compact Message Test:");
    println!("   Original size: {} bytes", msg.size());
    println!("   Serialized: {} bytes", bytes.len());
    println!(
        "   Decoded payload: {:?}",
        String::from_utf8_lossy(&decoded.payload)
    );
    println!(
        "   Header size: {} bytes",
        std::mem::size_of::<spine_agentic::CompactHeader>()
    );
}

fn demo_message_pool() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("DEMO 5: Zero-Copy Message Pool Efficiency");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let pool = MessagePool::new();

    // Allocate and return buffers
    let sizes = [100, 500, 1024, 4096, 16384, 65536];

    println!("🔄 Allocating buffers of various sizes:");
    for &size in &sizes {
        let mut buffer = pool.allocate(size);
        let test_data: Vec<u8> = (0..size.min(buffer.len()))
            .map(|i| (i % 256) as u8)
            .collect();
        buffer.write(&test_data);
        println!(
            "   Requested: {} bytes, Allocated: {} bytes",
            size,
            buffer.len()
        );
        // Buffer is returned to pool when dropped
    }

    // Allocate again to test reuse
    println!("\n🔄 Reallocating (testing pool reuse):");
    for &size in &sizes {
        let buffer = pool.allocate(size);
        println!(
            "   Requested: {} bytes, Got: {} bytes (reused)",
            size,
            buffer.len()
        );
    }

    // Pool stats
    let stats = pool.stats();
    println!("\n📊 Pool Statistics:");
    println!(
        "   Size classes: {:?}",
        stats
            .size_classes
            .iter()
            .map(|s| format!("{}B", s))
            .collect::<Vec<_>>()
    );
    println!("   Total buffers: {}", stats.total_buffers);
    println!("   Total pooled memory: {} bytes", stats.total_bytes);

    // Benchmark
    println!("\n⏱️  Performance Comparison:");

    let iterations = 10000;

    // Without pool
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let _buf: Vec<u8> = vec![0u8; 4096];
    }
    let no_pool_time = start.elapsed();

    // With pool
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let _buf = pool.allocate(4096);
    }
    let pool_time = start.elapsed();

    println!("   {} allocations of 4KB:", iterations);
    println!("   Without pool: {:?}", no_pool_time);
    println!("   With pool:    {:?}", pool_time);
    println!(
        "   Speedup: {:.1}x",
        no_pool_time.as_nanos() as f64 / pool_time.as_nanos() as f64
    );
}
