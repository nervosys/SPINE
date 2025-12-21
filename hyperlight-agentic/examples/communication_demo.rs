//! Communication Protocols Demo
//! 
//! Demonstrates the advanced agent communication systems:
//! - FIPA-style speech acts and performatives
//! - Message broker with pub/sub
//! - Contract Net Protocol for task delegation
//! - Blackboard architecture for collaborative problem solving
//! - Trust and reputation systems

use hyperlight_agentic::{
    // Communication protocols
    SpeechAct, Performative, MessageBroker, BrokerStats,
    // Contract Net
    TaskAnnouncement, ContractBid, ContractNetManager, ContractNetStats,
    // Blackboard
    Blackboard, KnowledgeSource, KnowledgeLevel, BlackboardStats,
    // Trust
    TrustSystem, InteractionType, InteractionOutcome, TrustStats,
    // Core types
    AgentCapability,
};
use uuid::Uuid;
use chrono::{Utc, Duration};
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║       HYPERLIGHT AGENT COMMUNICATION PROTOCOLS DEMO          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    demo_speech_acts_and_broker().await;
    demo_contract_net_protocol().await;
    demo_blackboard_architecture().await;
    demo_trust_and_reputation().await;

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║              ALL COMMUNICATION DEMOS COMPLETE                ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}

async fn demo_speech_acts_and_broker() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  DEMO 1: FIPA Speech Acts & Message Broker                  │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    // Create agents
    let agent_a = Uuid::new_v4();
    let agent_b = Uuid::new_v4();
    let agent_c = Uuid::new_v4();
    
    println!("  Agents created:");
    println!("    • Agent A (Coordinator): {}", &agent_a.to_string()[..8]);
    println!("    • Agent B (Worker): {}", &agent_b.to_string()[..8]);
    println!("    • Agent C (Observer): {}", &agent_c.to_string()[..8]);

    // Create message broker
    let broker = MessageBroker::new();
    
    // Register agents
    broker.register_agent(agent_a);
    broker.register_agent(agent_b);
    broker.register_agent(agent_c);
    println!("\n  Agents registered with broker");

    // Subscribe agent_c to updates topic
    broker.subscribe(agent_c, "task_updates");
    println!("  Agent C subscribed to 'task_updates' topic");

    // Create conversation
    let conversation_id = Uuid::new_v4();
    
    // Agent A requests work from Agent B
    let request = Performative::new(
        agent_a,
        vec![agent_b],
        SpeechAct::Request {
            action: "analyze_data".to_string(),
            parameters: {
                let mut params = HashMap::new();
                params.insert("dataset".to_string(), serde_json::json!("user_behavior_2024"));
                params.insert("depth".to_string(), serde_json::json!(3));
                params
            },
        },
    )
    .with_conversation(conversation_id)
    .with_protocol("task-delegation");
    
    let msg_id = broker.send(request).await.unwrap();
    println!("\n  Message flow:");
    println!("    1. Agent A → Agent B: Request (analyze_data)");
    println!("       Message ID: {}", &msg_id.to_string()[..8]);

    // Agent B receives and acknowledges
    let messages = broker.receive(&agent_b, 10).await;
    println!("    2. Agent B received {} message(s)", messages.len());
    
    // Agent B sends acknowledgment
    let ack = Performative::new(
        agent_b,
        vec![agent_a],
        SpeechAct::Acknowledge { message_id: msg_id },
    )
    .with_conversation(conversation_id)
    .reply_to(msg_id);
    
    broker.send(ack).await.unwrap();
    println!("    3. Agent B → Agent A: Acknowledge");

    // Agent B makes a promise
    let promise = Performative::new(
        agent_b,
        vec![agent_a],
        SpeechAct::Promise {
            action: "analyze_data".to_string(),
            deadline: Some(Utc::now() + Duration::hours(2)),
        },
    )
    .with_conversation(conversation_id);
    
    broker.send(promise).await.unwrap();
    println!("    4. Agent B → Agent A: Promise (deadline: 2 hours)");

    // Agent B informs about progress (also publishes to topic)
    let progress_info = Performative::new(
        agent_b,
        vec![agent_a],
        SpeechAct::Inform {
            content: serde_json::json!({
                "topic": "task_updates",
                "status": "in_progress",
                "progress": 50,
                "task": "analyze_data"
            }),
        },
    )
    .with_conversation(conversation_id);
    
    broker.send(progress_info).await.unwrap();
    println!("    5. Agent B → All subscribers: Inform (50% progress)");

    // Check Agent C received via subscription
    let c_messages = broker.receive(&agent_c, 10).await;
    println!("    6. Agent C received {} message(s) via subscription", c_messages.len());

    // Complete conversation
    broker.complete_conversation(&conversation_id, "Task delegated successfully");
    
    // Get conversation details
    if let Some(conv) = broker.get_conversation(&conversation_id) {
        println!("\n  Conversation Summary:");
        println!("    • ID: {}", &conv.id.to_string()[..8]);
        println!("    • Protocol: {:?}", conv.protocol);
        println!("    • Messages: {}", conv.messages.len());
        println!("    • State: {:?}", conv.state);
    }

    // Broker stats
    let stats: BrokerStats = broker.stats();
    println!("\n  Broker Statistics:");
    println!("    • Registered agents: {}", stats.registered_agents);
    println!("    • Active conversations: {}", stats.active_conversations);
    println!("    • Total conversations: {}", stats.total_conversations);
    println!("    • Subscription topics: {}", stats.subscription_topics);
    println!("    • Total messages: {}", stats.total_messages);

    println!("\n  ✓ Speech Acts & Message Broker demo complete\n");
}

async fn demo_contract_net_protocol() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  DEMO 2: Contract Net Protocol                              │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let manager = Uuid::new_v4();
    let contractor_1 = Uuid::new_v4();
    let contractor_2 = Uuid::new_v4();
    let contractor_3 = Uuid::new_v4();

    println!("  Participants:");
    println!("    • Manager: {}", &manager.to_string()[..8]);
    println!("    • Contractor 1: {}", &contractor_1.to_string()[..8]);
    println!("    • Contractor 2: {}", &contractor_2.to_string()[..8]);
    println!("    • Contractor 3: {}", &contractor_3.to_string()[..8]);

    let contract_net = ContractNetManager::new();

    // Manager announces a task
    let task = TaskAnnouncement {
        id: Uuid::new_v4(),
        manager,
        task_description: "Build recommendation engine for e-commerce platform".to_string(),
        requirements: vec![
            AgentCapability::ContinualLearning,
            AgentCapability::KnowledgeManagement,
        ],
        deadline: Utc::now() + Duration::days(30),
        bid_deadline: Utc::now() + Duration::days(7),
        eligibility_criteria: vec![
            "Must have ML experience".to_string(),
            "Must handle 1M+ users".to_string(),
        ],
        metadata: HashMap::new(),
    };

    let task_id = contract_net.announce_task(task);
    println!("\n  Task announced:");
    println!("    • Task ID: {}", &task_id.to_string()[..8]);
    println!("    • Description: Build recommendation engine");
    println!("    • Bid deadline: 7 days");

    // Contractors submit bids
    let bid_1 = ContractBid {
        id: Uuid::new_v4(),
        task_id,
        bidder: contractor_1,
        proposed_cost: 15000.0,
        proposed_duration: Duration::days(25).to_std().unwrap(),
        confidence: 0.85,
        approach: "Collaborative filtering with deep learning enhancements".to_string(),
        resources_required: vec!["GPU cluster".to_string(), "Data lake access".to_string()],
        submitted_at: Utc::now(),
    };
    contract_net.submit_bid(bid_1.clone()).unwrap();
    println!("\n  Bids submitted:");
    println!("    • Contractor 1: $15,000, 25 days, 85% confidence");

    let bid_2 = ContractBid {
        id: Uuid::new_v4(),
        task_id,
        bidder: contractor_2,
        proposed_cost: 12000.0,
        proposed_duration: Duration::days(28).to_std().unwrap(),
        confidence: 0.75,
        approach: "Hybrid content-based and collaborative filtering".to_string(),
        resources_required: vec!["ML pipeline".to_string()],
        submitted_at: Utc::now(),
    };
    contract_net.submit_bid(bid_2.clone()).unwrap();
    println!("    • Contractor 2: $12,000, 28 days, 75% confidence");

    let bid_3 = ContractBid {
        id: Uuid::new_v4(),
        task_id,
        bidder: contractor_3,
        proposed_cost: 18000.0,
        proposed_duration: Duration::days(20).to_std().unwrap(),
        confidence: 0.95,
        approach: "Transformer-based neural recommendation with real-time updates".to_string(),
        resources_required: vec!["High-memory GPUs".to_string(), "Streaming infrastructure".to_string()],
        submitted_at: Utc::now(),
    };
    contract_net.submit_bid(bid_3.clone()).unwrap();
    println!("    • Contractor 3: $18,000, 20 days, 95% confidence");

    // Get all bids
    let all_bids = contract_net.get_bids(&task_id);
    println!("\n  Total bids received: {}", all_bids.len());

    // Manager evaluates and awards contract (choosing highest confidence)
    let winning_bid = &bid_3;
    let contract = contract_net.award_contract(&task_id, &winning_bid.id).unwrap();
    
    println!("\n  Contract awarded:");
    println!("    • Contract ID: {}", &contract.id.to_string()[..8]);
    println!("    • Winner: Contractor 3");
    println!("    • Agreed cost: ${}", contract.agreed_cost);
    println!("    • Status: {:?}", contract.status);

    // Simulate progress updates
    println!("\n  Contract execution:");
    contract_net.update_progress(&contract.id, 25);
    println!("    • Progress: 25%");
    
    contract_net.update_progress(&contract.id, 50);
    println!("    • Progress: 50%");
    
    contract_net.update_progress(&contract.id, 75);
    println!("    • Progress: 75%");
    
    contract_net.complete_contract(&contract.id, "Recommendation engine deployed successfully");
    println!("    • Status: Completed");

    // Get contractor's contracts
    let contractor_contracts = contract_net.get_agent_contracts(&contractor_3);
    println!("\n  Contractor 3's contracts: {}", contractor_contracts.len());

    // Stats
    let stats: ContractNetStats = contract_net.stats();
    println!("\n  Contract Net Statistics:");
    println!("    • Open tasks: {}", stats.open_tasks);
    println!("    • Total bids: {}", stats.total_bids);
    println!("    • Active contracts: {}", stats.active_contracts);
    println!("    • Completed contracts: {}", stats.completed_contracts);
    println!("    • Failed contracts: {}", stats.failed_contracts);

    println!("\n  ✓ Contract Net Protocol demo complete\n");
}

async fn demo_blackboard_architecture() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  DEMO 3: Blackboard Architecture                            │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let blackboard = Blackboard::new();
    
    let sensor_agent = Uuid::new_v4();
    let feature_agent = Uuid::new_v4();
    let hypothesis_agent = Uuid::new_v4();
    let solution_agent = Uuid::new_v4();

    println!("  Knowledge Sources:");
    println!("    • Sensor Agent: {}", &sensor_agent.to_string()[..8]);
    println!("    • Feature Agent: {}", &feature_agent.to_string()[..8]);
    println!("    • Hypothesis Agent: {}", &hypothesis_agent.to_string()[..8]);
    println!("    • Solution Agent: {}", &solution_agent.to_string()[..8]);

    // Register knowledge sources
    let ks_sensor = KnowledgeSource::new("SensorProcessor", vec![], KnowledgeLevel::Raw);
    let ks_feature = KnowledgeSource::new("FeatureExtractor", vec![KnowledgeLevel::Raw], KnowledgeLevel::Feature);
    let ks_hypothesis = KnowledgeSource::new("HypothesisGenerator", vec![KnowledgeLevel::Feature], KnowledgeLevel::Hypothesis);
    let ks_solution = KnowledgeSource::new("SolutionBuilder", vec![KnowledgeLevel::Hypothesis], KnowledgeLevel::Solution);

    blackboard.register_source(ks_sensor);
    blackboard.register_source(ks_feature);
    blackboard.register_source(ks_hypothesis);
    blackboard.register_source(ks_solution);
    println!("\n  Knowledge sources registered");

    // Watch for solutions
    blackboard.watch("final_solution", solution_agent);
    println!("  Solution agent watching for 'final_solution'\n");

    // Simulate collaborative problem solving
    println!("  Problem: Identify user intent from multi-modal input\n");

    // Step 1: Raw data
    blackboard.write(
        "raw_text",
        serde_json::json!({
            "content": "I need to book a flight to New York",
            "timestamp": Utc::now().to_rfc3339(),
            "source": "voice_transcription"
        }),
        sensor_agent,
        KnowledgeLevel::Raw,
    ).await;
    println!("  Step 1: Raw text added to blackboard");

    blackboard.write(
        "raw_gesture",
        serde_json::json!({
            "gesture": "pointing_at_calendar",
            "confidence": 0.92
        }),
        sensor_agent,
        KnowledgeLevel::Raw,
    ).await;
    println!("  Step 2: Raw gesture added to blackboard");

    // Step 2: Features extracted
    blackboard.write(
        "intent_keywords",
        serde_json::json!({
            "keywords": ["book", "flight", "New York"],
            "entities": [
                {"type": "action", "value": "book"},
                {"type": "object", "value": "flight"},
                {"type": "destination", "value": "New York"}
            ]
        }),
        feature_agent,
        KnowledgeLevel::Feature,
    ).await;
    println!("  Step 3: Keywords extracted to feature level");

    blackboard.write(
        "temporal_context",
        serde_json::json!({
            "implied_time": "near_future",
            "gesture_context": "calendar_interaction",
            "urgency": "medium"
        }),
        feature_agent,
        KnowledgeLevel::Feature,
    ).await;
    println!("  Step 4: Temporal context extracted");

    // Step 3: Hypothesis
    blackboard.write(
        "intent_hypothesis",
        serde_json::json!({
            "primary_intent": "flight_booking",
            "confidence": 0.88,
            "supporting_evidence": ["text_keywords", "gesture_calendar"],
            "destination": "New York",
            "timeframe": "within_week"
        }),
        hypothesis_agent,
        KnowledgeLevel::Hypothesis,
    ).await;
    println!("  Step 5: Intent hypothesis generated");

    // Step 4: Solution
    blackboard.write(
        "final_solution",
        serde_json::json!({
            "action": "initiate_flight_booking_flow",
            "parameters": {
                "destination": "New York, NY",
                "departure_window": "next_7_days",
                "return_window": "flexible"
            },
            "confidence": 0.91,
            "fallback_action": "clarify_dates"
        }),
        solution_agent,
        KnowledgeLevel::Solution,
    ).await;
    println!("  Step 6: Solution generated\n");

    // Read back the solution
    if let Some(solution) = blackboard.read("final_solution") {
        println!("  Final Solution:");
        println!("    • Action: initiate_flight_booking_flow");
        println!("    • Confidence: {:?}", solution.value.get("confidence"));
        println!("    • Author: {}", &solution.author.to_string()[..8]);
        println!("    • Version: {}", solution.version);
    }

    // Query by level
    let hypotheses = blackboard.read_level(&KnowledgeLevel::Hypothesis);
    let features = blackboard.read_level(&KnowledgeLevel::Feature);
    println!("\n  Knowledge by Level:");
    println!("    • Raw entries: {}", blackboard.read_level(&KnowledgeLevel::Raw).len());
    println!("    • Feature entries: {}", features.len());
    println!("    • Hypothesis entries: {}", hypotheses.len());
    println!("    • Solution entries: {}", blackboard.read_level(&KnowledgeLevel::Solution).len());

    // Get recent changes
    let changes = blackboard.get_changes(Utc::now() - Duration::minutes(5)).await;
    println!("\n  Recent changes: {} modifications", changes.len());

    // Stats
    let stats: BlackboardStats = blackboard.stats();
    println!("\n  Blackboard Statistics:");
    println!("    • Total entries: {}", stats.total_entries);
    println!("    • Knowledge sources: {}", stats.knowledge_sources);
    println!("    • Watchers: {}", stats.watchers);

    println!("\n  ✓ Blackboard Architecture demo complete\n");
}

async fn demo_trust_and_reputation() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  DEMO 4: Trust & Reputation System                          │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let agent_alice = Uuid::new_v4();
    let agent_bob = Uuid::new_v4();
    let agent_charlie = Uuid::new_v4();
    let agent_eve = Uuid::new_v4();

    println!("  Agents:");
    println!("    • Alice (Reliable): {}", &agent_alice.to_string()[..8]);
    println!("    • Bob (Competent): {}", &agent_bob.to_string()[..8]);
    println!("    • Charlie (Average): {}", &agent_charlie.to_string()[..8]);
    println!("    • Eve (Deceptive): {}", &agent_eve.to_string()[..8]);

    let trust_system = TrustSystem::new(0.01); // 1% decay rate

    // Simulate interactions
    println!("\n  Recording interactions...\n");

    // Alice - consistently reliable
    for i in 0..5 {
        trust_system.record_interaction(
            agent_bob,
            agent_alice,
            InteractionType::TaskDelegation,
            InteractionOutcome::Success { quality: 0.9 + (i as f64 * 0.02) },
        );
    }
    println!("    Alice: 5 successful task delegations (high quality)");

    // Bob - competent but sometimes late
    trust_system.record_interaction(
        agent_alice,
        agent_bob,
        InteractionType::Collaboration,
        InteractionOutcome::Success { quality: 0.95 },
    );
    trust_system.record_interaction(
        agent_alice,
        agent_bob,
        InteractionType::Collaboration,
        InteractionOutcome::PartialSuccess { completion: 0.8 },
    );
    trust_system.record_interaction(
        agent_alice,
        agent_bob,
        InteractionType::ContractExecution,
        InteractionOutcome::Success { quality: 0.85 },
    );
    println!("    Bob: 2 successful + 1 partial success collaborations");

    // Charlie - mixed results
    trust_system.record_interaction(
        agent_alice,
        agent_charlie,
        InteractionType::InformationSharing,
        InteractionOutcome::Success { quality: 0.7 },
    );
    trust_system.record_interaction(
        agent_bob,
        agent_charlie,
        InteractionType::ResourceSharing,
        InteractionOutcome::Failure { severity: 0.3 },
    );
    trust_system.record_interaction(
        agent_alice,
        agent_charlie,
        InteractionType::TaskDelegation,
        InteractionOutcome::Timeout,
    );
    println!("    Charlie: 1 success, 1 failure, 1 timeout");

    // Eve - deceptive behavior
    trust_system.record_interaction(
        agent_alice,
        agent_eve,
        InteractionType::InformationSharing,
        InteractionOutcome::Deception,
    );
    trust_system.record_interaction(
        agent_bob,
        agent_eve,
        InteractionType::ContractExecution,
        InteractionOutcome::Deception,
    );
    println!("    Eve: 2 deceptive interactions detected");

    // Get trust assessments
    println!("\n  Trust Assessments (from Alice's perspective):");
    
    if let Some(trust_bob) = trust_system.get_trust(&agent_alice, &agent_bob) {
        println!("    Alice → Bob:");
        println!("      • Overall trust: {:.2}", trust_bob.overall_trust);
        println!("      • Competence: {:.2}", trust_bob.competence);
        println!("      • Reliability: {:.2}", trust_bob.reliability);
        println!("      • Interactions: {}", trust_bob.interaction_count);
    }

    if let Some(trust_charlie) = trust_system.get_trust(&agent_alice, &agent_charlie) {
        println!("    Alice → Charlie:");
        println!("      • Overall trust: {:.2}", trust_charlie.overall_trust);
        println!("      • Competence: {:.2}", trust_charlie.competence);
        println!("      • Reliability: {:.2}", trust_charlie.reliability);
    }

    if let Some(trust_eve) = trust_system.get_trust(&agent_alice, &agent_eve) {
        println!("    Alice → Eve:");
        println!("      • Overall trust: {:.2}", trust_eve.overall_trust);
        println!("      • Honesty: {:.2}", trust_eve.honesty);
        println!("      • ⚠️  Low trust due to deception");
    }

    // Add endorsements and warnings
    println!("\n  Reputation Actions:");
    
    trust_system.endorse(
        agent_alice,
        agent_bob,
        "machine_learning",
        "Excellent ML expertise, delivered high-quality models",
    );
    println!("    • Alice endorsed Bob for machine_learning");

    trust_system.endorse(
        agent_charlie,
        agent_alice,
        "reliability",
        "Always delivers on time with great communication",
    );
    println!("    • Charlie endorsed Alice for reliability");

    trust_system.warn(
        agent_alice,
        agent_eve,
        "Provided falsified analysis results",
        0.8,
    );
    println!("    • Alice warned about Eve (severity: 0.8)");

    // Compute reputations
    let rep_alice = trust_system.compute_reputation(&agent_alice);
    let rep_bob = trust_system.compute_reputation(&agent_bob);
    let rep_eve = trust_system.compute_reputation(&agent_eve);

    println!("\n  Global Reputation Scores:");
    println!("    • Alice: {:.2} (endorsements: {})", rep_alice.global_score, rep_alice.endorsements.len());
    println!("    • Bob: {:.2} (endorsements: {})", rep_bob.global_score, rep_bob.endorsements.len());
    println!("    • Eve: {:.2} (warnings: {}) ⚠️", rep_eve.global_score, rep_eve.warnings.len());

    // Apply decay
    trust_system.apply_decay();
    println!("\n  Trust decay applied (simulating time passage)");

    // Stats
    let stats: TrustStats = trust_system.stats();
    println!("\n  Trust System Statistics:");
    println!("    • Total assessments: {}", stats.total_assessments);
    println!("    • Average trust: {:.2}", stats.average_trust);
    println!("    • Total interactions: {}", stats.total_interactions);
    println!("    • Agents with reputation: {}", stats.agents_with_reputation);

    println!("\n  ✓ Trust & Reputation System demo complete\n");
}
