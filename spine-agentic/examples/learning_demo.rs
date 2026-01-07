//! Learning & Adaptation Demo
//! 
//! Demonstrates the learning systems in hyperlight-agentic:
//! - Agent reinforcement learning with Q-learning
//! - Skill library with transfer learning
//! - Meta-learning for rapid adaptation
//! - Curriculum learning with progressive difficulty
//! - Emergent behavior detection

use spine_agentic::{
    // Learning
    AgentLearner, LearningAlgorithm, Experience, StateRepresentation, LearningSignal,
    // Skills
    SkillLibrary, Skill, SkillCategory, SkillCondition, ConditionKind, SkillEffect, EffectKind, SkillParameter, SkillParamType,
    // Meta-learning
    MetaLearner, MetaLearningConfig, LearningTask,
    // Curriculum
    CurriculumManager, CurriculumStage,
    // Emergent behavior
    EmergentBehaviorDetector, AgentAction,
};
use uuid::Uuid;
use chrono::Utc;
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                    LEARNING & ADAPTATION DEMO");
    println!("═══════════════════════════════════════════════════════════════════\n");

    demo_reinforcement_learning().await;
    demo_skill_library();
    demo_meta_learning().await;
    demo_curriculum_learning();
    demo_emergent_behavior().await;

    println!("\n═══════════════════════════════════════════════════════════════════");
    println!("                     LEARNING DEMO COMPLETE");
    println!("═══════════════════════════════════════════════════════════════════");
}

async fn demo_reinforcement_learning() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│              DEMO 1: REINFORCEMENT LEARNING                     │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    // Create an agent learner with Q-learning
    let agent_id = Uuid::new_v4();
    let mut learner = AgentLearner::new(
        agent_id,
        LearningAlgorithm::QLearning { alpha: 0.1, gamma: 0.99 },
        1000, // Buffer capacity
    );
    
    println!("  Created Q-Learning agent with:");
    println!("    • Learning rate (α): 0.1");
    println!("    • Discount factor (γ): 0.99");
    println!("    • Experience buffer capacity: 1000\n");

    // Define available actions for web navigation domain
    let actions = vec![
        "navigation.click".to_string(),
        "navigation.scroll".to_string(),
        "navigation.back".to_string(),
        "navigation.forward".to_string(),
        "data.extract".to_string(),
        "data.submit".to_string(),
    ];

    println!("  Training on simulated navigation experiences...\n");

    // Simulate experiences
    let experiences = vec![
        ("navigation.click", 1.0, "Clicked login button successfully"),
        ("navigation.scroll", 0.2, "Scrolled to find content"),
        ("navigation.click", 0.8, "Clicked product link"),
        ("data.extract", 1.5, "Extracted product price"),
        ("navigation.back", -0.1, "Went back unnecessarily"),
        ("data.submit", 2.0, "Submitted order successfully"),
        ("navigation.click", 0.5, "Clicked checkout"),
        ("data.extract", 1.0, "Extracted order confirmation"),
    ];

    for (action, reward, context) in &experiences {
        let exp = Experience {
            id: Uuid::new_v4(),
            state: StateRepresentation::new(vec![0.5, 0.3, 0.8])
                .with_symbol("page", serde_json::json!("shopping")),
            action: action.to_string(),
            next_state: StateRepresentation::new(vec![0.6, 0.4, 0.7]),
            signal: LearningSignal::Reward { 
                value: *reward, 
                context: context.to_string() 
            },
            priority: reward.abs(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        };
        learner.record_experience(exp).await;
    }

    println!("  Recorded {} experiences\n", experiences.len());

    // Learn from experiences
    let avg_delta = learner.learn(8, "navigation").await;
    println!("  Learning update:");
    println!("    • Average TD error: {:.4}", avg_delta);

    // Select actions using learned policy
    println!("\n  Action selection (ε-greedy):");
    for _ in 0..3 {
        let action = learner.select_action("navigation", &actions);
        println!("    → Selected: {}", action);
    }

    // Decay exploration
    learner.decay_exploration("navigation", 0.9, 0.05);
    
    let stats = learner.get_stats();
    println!("\n  Learning Statistics:");
    println!("    • Total experiences: {}", stats.total_experiences);
    println!("    • Cumulative reward: {:.2}", stats.cumulative_reward);
    println!("    • Policy count: {}", stats.policy_count);
    println!("    • Average action value: {:.4}\n", stats.average_value);
}

fn demo_skill_library() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                   DEMO 2: SKILL LIBRARY                         │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let library = SkillLibrary::new();

    // Register skills
    let navigate_skill = Skill {
        id: Uuid::new_v4(),
        name: "web_navigate".to_string(),
        description: "Navigate to a URL and wait for page load".to_string(),
        category: SkillCategory::Navigation,
        preconditions: vec![
            SkillCondition {
                kind: ConditionKind::HasCapability("browser".to_string()),
                parameters: HashMap::new(),
            }
        ],
        effects: vec![
            SkillEffect {
                kind: EffectKind::StateChange { 
                    key: "current_page".to_string(), 
                    value: serde_json::json!("target_url") 
                },
                probability: 0.95,
            }
        ],
        parameters: vec![
            SkillParameter {
                name: "url".to_string(),
                param_type: SkillParamType::Url,
                default: None,
                required: true,
            }
        ],
        execution_trace: Some("fetch_page -> wait_ready -> verify_load".to_string()),
        success_rate: 0.95,
        usage_count: 0,
        created_at: Utc::now(),
        learned_from: None,
    };
    
    let extract_skill = Skill {
        id: Uuid::new_v4(),
        name: "data_extract".to_string(),
        description: "Extract structured data using CSS selectors".to_string(),
        category: SkillCategory::DataExtraction,
        preconditions: vec![
            SkillCondition {
                kind: ConditionKind::StateEquals { 
                    key: "page_loaded".to_string(), 
                    value: serde_json::json!(true) 
                },
                parameters: HashMap::new(),
            }
        ],
        effects: vec![
            SkillEffect {
                kind: EffectKind::KnowledgeGained { topic: "extracted_data".to_string() },
                probability: 0.9,
            }
        ],
        parameters: vec![
            SkillParameter {
                name: "selector".to_string(),
                param_type: SkillParamType::Selector,
                default: None,
                required: true,
            }
        ],
        execution_trace: Some("query_selector -> extract_text -> validate_data".to_string()),
        success_rate: 0.88,
        usage_count: 0,
        created_at: Utc::now(),
        learned_from: None,
    };

    let analyze_skill = Skill {
        id: Uuid::new_v4(),
        name: "analyze_content".to_string(),
        description: "Analyze page content for patterns and insights".to_string(),
        category: SkillCategory::Analysis,
        preconditions: vec![],
        effects: vec![
            SkillEffect {
                kind: EffectKind::KnowledgeGained { topic: "content_analysis".to_string() },
                probability: 0.85,
            }
        ],
        parameters: vec![],
        execution_trace: Some("tokenize -> classify -> summarize".to_string()),
        success_rate: 0.82,
        usage_count: 0,
        created_at: Utc::now(),
        learned_from: None,
    };

    println!("  Registering skills:");
    let nav_id = library.register(navigate_skill);
    println!("    ✓ web_navigate (Navigation)");
    let extract_id = library.register(extract_skill);
    println!("    ✓ data_extract (DataExtraction)");
    library.register(analyze_skill);
    println!("    ✓ analyze_content (Analysis)\n");

    // Query skills
    println!("  Skills by category (Navigation):");
    for skill in library.find_by_category(&SkillCategory::Navigation) {
        println!("    • {} (success: {:.0}%)", skill.name, skill.success_rate * 100.0);
    }

    // Find applicable skills
    let mut current_state = HashMap::new();
    current_state.insert("page_loaded".to_string(), serde_json::json!(true));
    current_state.insert("capabilities".to_string(), serde_json::json!(["browser", "extractor"]));
    
    println!("\n  Applicable skills for current state:");
    for skill in library.find_applicable(&current_state) {
        println!("    • {} - {}", skill.name, skill.description);
    }

    // Skill transfer
    println!("\n  Skill transfer:");
    let target_agent = Uuid::new_v4();
    if let Some(transferred) = library.transfer_skill(&nav_id, target_agent) {
        println!("    ✓ Transferred 'web_navigate' to new agent");
        println!("    Initial success rate: {:.0}% (20% penalty applied)", transferred.success_rate * 100.0);
    }

    // Record usage
    library.record_usage(&nav_id, true);
    library.record_usage(&nav_id, true);
    library.record_usage(&extract_id, true);
    library.record_usage(&extract_id, false);
    
    println!("\n  Updated usage statistics:");
    if let Some(skill) = library.get(&nav_id) {
        println!("    • web_navigate: {} uses, {:.0}% success", skill.usage_count, skill.success_rate * 100.0);
    }
    if let Some(skill) = library.get(&extract_id) {
        println!("    • data_extract: {} uses, {:.0}% success", skill.usage_count, skill.success_rate * 100.0);
    }

    let stats = library.stats();
    println!("\n  Library Statistics:");
    println!("    • Total skills: {}", stats.total_skills);
    println!("    • Average success rate: {:.1}%", stats.average_success_rate * 100.0);
    println!("    • Total usages: {}\n", stats.total_usages);
}

async fn demo_meta_learning() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                   DEMO 3: META-LEARNING                         │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    // Create meta-learner with MAML-like configuration
    let config = MetaLearningConfig {
        inner_learning_rate: 0.01,
        outer_learning_rate: 0.001,
        inner_steps: 5,
        task_batch_size: 4,
        adaptation_steps: 10,
    };
    
    println!("  Meta-Learner Configuration:");
    println!("    • Inner learning rate: {}", config.inner_learning_rate);
    println!("    • Outer learning rate: {}", config.outer_learning_rate);
    println!("    • Adaptation steps: {}\n", config.adaptation_steps);

    let mut meta_learner = MetaLearner::new(config);

    // Create sample tasks
    let tasks = vec![
        create_learning_task("news_scraping", "web_extraction"),
        create_learning_task("form_filling", "web_interaction"),
        create_learning_task("price_monitoring", "web_extraction"),
        create_learning_task("login_automation", "web_interaction"),
    ];

    println!("  Training tasks:");
    for task in &tasks {
        println!("    • {} (domain: {}, difficulty: {:.1})", 
                 task.name, task.domain, task.difficulty);
    }

    // Perform meta-update
    println!("\n  Performing meta-update across task batch...");
    meta_learner.meta_update(&tasks).await;

    // Demonstrate rapid adaptation
    println!("\n  Rapid adaptation to new task:");
    let new_task = create_learning_task("product_search", "web_extraction");
    println!("    New task: {} (domain: {})", new_task.name, new_task.domain);
    
    let adapted_policy = meta_learner.adapt_to_task(&new_task).await;
    println!("    ✓ Adapted policy: {}", adapted_policy.name);
    println!("    Exploration rate: {:.2}", adapted_policy.exploration_rate);

    // Record task for future meta-updates
    meta_learner.record_task(new_task);

    let stats = meta_learner.stats();
    println!("\n  Meta-Learning Statistics:");
    println!("    • Tasks learned: {}", stats.tasks_learned);
    println!("    • Domains covered: {:?}", stats.domains);
    println!("    • Meta-parameter norm: {:.4}\n", stats.meta_parameter_norm);
}

fn create_learning_task(name: &str, domain: &str) -> LearningTask {
    // Create synthetic experiences for the task
    let train_experiences: Vec<Experience> = (0..5).map(|i| {
        Experience {
            id: Uuid::new_v4(),
            state: StateRepresentation::new(vec![i as f64 * 0.1, 0.5, 0.3]),
            action: format!("{}.action_{}", domain, i),
            next_state: StateRepresentation::new(vec![(i + 1) as f64 * 0.1, 0.6, 0.4]),
            signal: LearningSignal::Reward { 
                value: 0.5 + (i as f64 * 0.1), 
                context: format!("training_{}", i) 
            },
            priority: 1.0,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }).collect();

    let test_experiences: Vec<Experience> = (0..3).map(|i| {
        Experience {
            id: Uuid::new_v4(),
            state: StateRepresentation::new(vec![0.8, 0.4, 0.6]),
            action: format!("{}.action_{}", domain, i),
            next_state: StateRepresentation::new(vec![0.9, 0.5, 0.7]),
            signal: LearningSignal::Reward { 
                value: 0.7 + (i as f64 * 0.05), 
                context: format!("test_{}", i) 
            },
            priority: 1.0,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }).collect();

    LearningTask {
        id: Uuid::new_v4(),
        name: name.to_string(),
        domain: domain.to_string(),
        train_experiences,
        test_experiences,
        difficulty: 0.5,
        similarity_to_prior: 0.7,
    }
}

fn demo_curriculum_learning() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│                DEMO 4: CURRICULUM LEARNING                      │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let mut curriculum = CurriculumManager::new();

    // Define curriculum stages (progressive difficulty)
    let stage1 = CurriculumStage {
        id: Uuid::new_v4(),
        name: "Basic Navigation".to_string(),
        description: "Learn to navigate between pages".to_string(),
        difficulty: 0.2,
        prerequisites: vec![],
        skills_taught: vec!["navigate".to_string(), "click".to_string()],
        success_threshold: 0.7,
        max_attempts: 3,
    };
    
    let stage1_id = stage1.id;
    
    let stage2 = CurriculumStage {
        id: Uuid::new_v4(),
        name: "Data Extraction".to_string(),
        description: "Learn to extract data from pages".to_string(),
        difficulty: 0.4,
        prerequisites: vec![stage1_id],
        skills_taught: vec!["select".to_string(), "extract".to_string()],
        success_threshold: 0.75,
        max_attempts: 3,
    };
    
    let stage2_id = stage2.id;
    
    let stage3 = CurriculumStage {
        id: Uuid::new_v4(),
        name: "Form Interaction".to_string(),
        description: "Learn to fill and submit forms".to_string(),
        difficulty: 0.6,
        prerequisites: vec![stage1_id],
        skills_taught: vec!["input".to_string(), "submit".to_string()],
        success_threshold: 0.8,
        max_attempts: 3,
    };
    
    let stage3_id = stage3.id;
    
    let stage4 = CurriculumStage {
        id: Uuid::new_v4(),
        name: "Complex Workflows".to_string(),
        description: "Combine skills into complex automation".to_string(),
        difficulty: 0.9,
        prerequisites: vec![stage2_id, stage3_id],
        skills_taught: vec!["orchestrate".to_string(), "recover".to_string()],
        success_threshold: 0.85,
        max_attempts: 5,
    };

    println!("  Building curriculum:");
    curriculum.add_stage(stage1);
    println!("    1. Basic Navigation (difficulty: 0.2)");
    curriculum.add_stage(stage2);
    println!("    2. Data Extraction (difficulty: 0.4)");
    curriculum.add_stage(stage3);
    println!("    3. Form Interaction (difficulty: 0.6)");
    curriculum.add_stage(stage4);
    println!("    4. Complex Workflows (difficulty: 0.9)");
    
    curriculum.sort_by_difficulty();
    println!("    ✓ Sorted by difficulty\n");

    // Enroll agents
    let agent1 = Uuid::new_v4();
    let agent2 = Uuid::new_v4();
    curriculum.enroll(agent1);
    curriculum.enroll(agent2);
    println!("  Enrolled 2 agents in curriculum\n");

    // Simulate agent1 progress
    println!("  Agent 1 progress:");
    
    if let Some(stage) = curriculum.get_next_stage(&agent1) {
        println!("    → Starting: {}", stage.name);
        
        // First attempt - fail
        let result = curriculum.record_attempt(&agent1, &stage.id, 0.5);
        match result {
            spine_agentic::CurriculumResult::RetryNeeded { attempts_remaining, current_score, .. } => {
                println!("    ✗ Attempt 1: {:.0}% (need {:.0}%, {} tries left)", 
                         current_score * 100.0, stage.success_threshold * 100.0, attempts_remaining);
            }
            _ => {}
        }
        
        // Second attempt - success
        let result = curriculum.record_attempt(&agent1, &stage.id, 0.85);
        match result {
            spine_agentic::CurriculumResult::StageCompleted { score, next_stage, .. } => {
                println!("    ✓ Attempt 2: {:.0}% - PASSED!", score * 100.0);
                if let Some(next) = next_stage {
                    if let Some(stage) = curriculum.get_next_stage(&agent1) {
                        println!("    → Next stage: {}", stage.name);
                    }
                }
            }
            _ => {}
        }
    }

    // Simulate agent2 getting stuck
    println!("\n  Agent 2 progress:");
    if let Some(stage) = curriculum.get_next_stage(&agent2) {
        println!("    → Starting: {}", stage.name);
        
        for i in 1..=3 {
            let result = curriculum.record_attempt(&agent2, &stage.id, 0.4);
            match result {
                spine_agentic::CurriculumResult::RetryNeeded { attempts_remaining, .. } => {
                    println!("    ✗ Attempt {}: 40% ({} tries left)", i, attempts_remaining);
                }
                spine_agentic::CurriculumResult::StageFailed { attempts, best_score, .. } => {
                    println!("    ✗ Attempt {}: FAILED after {} attempts (best: {:.0}%)", 
                             i, attempts, best_score * 100.0);
                }
                _ => {}
            }
        }
    }

    let stats = curriculum.stats();
    println!("\n  Curriculum Statistics:");
    println!("    • Total stages: {}", stats.total_stages);
    println!("    • Enrolled agents: {}", stats.enrolled_agents);
    println!("    • Average progress: {:.1}%\n", stats.average_progress * 100.0);
}

async fn demo_emergent_behavior() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│              DEMO 5: EMERGENT BEHAVIOR DETECTION                │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let detector = EmergentBehaviorDetector::new(50); // 50 action window

    // Create some agent IDs
    let agents: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();

    println!("  Simulating multi-agent activity...\n");

    // Simulate coordinated behavior (agents acting together)
    let base_time = Utc::now();
    for i in 0..30 {
        let agent_idx = i % agents.len();
        let action_type = if i < 10 {
            "navigate"
        } else if i < 20 {
            "extract"
        } else {
            "coordinate"
        };

        let action = AgentAction {
            agent_id: agents[agent_idx],
            action: format!("{}.action_{}", action_type, i % 5),
            timestamp: base_time + chrono::Duration::seconds(i as i64),
            context: {
                let mut ctx = HashMap::new();
                ctx.insert("page".to_string(), serde_json::json!(format!("page_{}", i % 3)));
                ctx
            },
            outcome: Some(0.7 + (i as f64 % 5.0) * 0.1),
        };
        
        detector.record_action(action).await;
    }

    println!("  Recorded 30 actions across 5 agents");

    // Analyze for emergent behaviors
    println!("\n  Analyzing for emergent behaviors...");
    let behaviors = detector.analyze().await;

    println!("\n  Detected behaviors:");
    for behavior in &behaviors {
        println!("    • {} ({} agents involved)", 
                 behavior.name, 
                 behavior.involved_agents.len());
        println!("      Description: {}", behavior.description);
        match &behavior.pattern {
            spine_agentic::BehaviorPattern::SpontaneousCoordination { action_sequence } => {
                println!("      Actions: {:?}", &action_sequence[..action_sequence.len().min(3)]);
            }
            spine_agentic::BehaviorPattern::RoleDifferentiation { roles, .. } => {
                println!("      Roles: {:?}", roles);
            }
            spine_agentic::BehaviorPattern::NovelStrategy { solution_path, .. } => {
                println!("      Strategy: {:?}", &solution_path[..solution_path.len().min(3)]);
            }
            _ => {}
        }
    }

    // Classify behaviors
    if !behaviors.is_empty() {
        detector.classify_behavior(&behaviors[0].id, true);
        println!("\n  Classified first behavior as beneficial");
    }

    let stats = detector.stats();
    println!("\n  Detection Statistics:");
    println!("    • Total detected: {}", stats.total_detected);
    println!("    • Beneficial: {}", stats.beneficial);
    println!("    • Harmful: {}", stats.harmful);
    println!("    • Unclassified: {}", stats.unclassified);
    println!("    • Total occurrences: {}\n", stats.total_occurrences);
}
