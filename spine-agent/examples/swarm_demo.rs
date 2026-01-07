use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

/// Simulated Node in the Swarm
#[derive(Clone, Debug)]
struct SwarmNode {
    id: Uuid,
    name: String,
    skills: Vec<String>,
    current_task: Option<Uuid>,
}

/// Simulated Task
#[derive(Clone, Debug)]
struct PlanTask {
    id: Uuid,
    description: String,
    required_skills: Vec<String>,
    dependencies: Vec<Uuid>,
    assigned_to: Option<Uuid>,
    status: TaskStatus,
}

#[derive(Clone, Debug, PartialEq)]
enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     🚀 SPINE swarm PLANNING DEMONSTRATION 🚀            ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Demonstrating: Skill-Based Task Allocation & Dependencies   ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // 1. Create simulated swarm nodes with different skills
    let nodes = vec![
        SwarmNode {
            id: Uuid::new_v4(),
            name: "Node-Alpha".to_string(),
            skills: vec!["research".to_string(), "scraping".to_string()],
            current_task: None,
        },
        SwarmNode {
            id: Uuid::new_v4(),
            name: "Node-Beta".to_string(),
            skills: vec!["synthesis".to_string(), "writing".to_string()],
            current_task: None,
        },
        SwarmNode {
            id: Uuid::new_v4(),
            name: "Node-Gamma".to_string(),
            skills: vec!["crypto".to_string(), "analysis".to_string()],
            current_task: None,
        },
    ];

    println!("📡 SWARM CLUSTER INITIALIZED");
    println!("───────────────────────────────────────────────────────────────");
    for node in &nodes {
        println!("  🖥️  {} | Skills: {:?}", node.name, node.skills);
    }
    println!();

    // 2. Define the goal and generate tasks
    let goal = "Analyze quantum computing impact on encryption and propose mitigation";
    println!("🎯 GOAL: \"{}\"\n", goal);

    let task1_id = Uuid::new_v4();
    let task2_id = Uuid::new_v4();
    let task3_id = Uuid::new_v4();

    let mut tasks = vec![
        PlanTask {
            id: task1_id,
            description: "Research current quantum computing capabilities".to_string(),
            required_skills: vec!["research".to_string(), "scraping".to_string()],
            dependencies: vec![],
            assigned_to: None,
            status: TaskStatus::Pending,
        },
        PlanTask {
            id: task2_id,
            description: "Analyze cryptographic vulnerabilities".to_string(),
            required_skills: vec!["crypto".to_string(), "analysis".to_string()],
            dependencies: vec![task1_id], // Depends on research
            assigned_to: None,
            status: TaskStatus::Pending,
        },
        PlanTask {
            id: task3_id,
            description: "Synthesize findings into mitigation report".to_string(),
            required_skills: vec!["synthesis".to_string(), "writing".to_string()],
            dependencies: vec![task1_id, task2_id], // Depends on both
            assigned_to: None,
            status: TaskStatus::Pending,
        },
    ];

    println!("📝 SWARM PLAN GENERATED ({} tasks)", tasks.len());
    println!("───────────────────────────────────────────────────────────────");
    for task in &tasks {
        let deps = if task.dependencies.is_empty() {
            "None".to_string()
        } else {
            format!("{} task(s)", task.dependencies.len())
        };
        println!("  📌 Task: {}", task.description);
        println!("     Required Skills: {:?}", task.required_skills);
        println!("     Dependencies: {}\n", deps);
    }

    // 3. Run the scheduler simulation
    println!("⚡ SCHEDULER STARTING...\n");
    println!("═══════════════════════════════════════════════════════════════");
    
    let mut tick = 0;
    let mut nodes = nodes;
    
    loop {
        tick += 1;
        println!("\n🕐 [Tick {}]", tick);
        
        // Collect completed task IDs
        let completed_tasks: Vec<Uuid> = tasks.iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .map(|t| t.id)
            .collect();

        let mut any_pending = false;
        
        for task in tasks.iter_mut() {
            if task.status == TaskStatus::Pending && task.assigned_to.is_none() {
                any_pending = true;
                
                // Check dependencies
                let deps_met = task.dependencies.iter().all(|dep_id| {
                    completed_tasks.contains(dep_id)
                });

                if !deps_met {
                    println!("   ⏳ '{}' - waiting on dependencies", task.description);
                    continue;
                }

                // SKILL-BASED ROUTING: Find best node
                let mut best_node: Option<(usize, usize)> = None; // (index, score)
                
                for (idx, node) in nodes.iter().enumerate() {
                    if node.current_task.is_some() {
                        continue; // Node is busy
                    }
                    
                    // Calculate skill match score
                    let score = task.required_skills.iter()
                        .filter(|s| node.skills.contains(s))
                        .count();
                    
                    if score > 0 && score > best_node.map(|(_, s)| s).unwrap_or(0) {
                        best_node = Some((idx, score));
                    }
                }

                if let Some((node_idx, score)) = best_node {
                    let node = &mut nodes[node_idx];
                    println!("   ✅ ASSIGNING: '{}'", task.description);
                    println!("      → {} (matched {}/{} skills)", 
                        node.name, score, task.required_skills.len());
                    
                    task.assigned_to = Some(node.id);
                    task.status = TaskStatus::InProgress;
                    node.current_task = Some(task.id);
                } else {
                    println!("   ⚠️  No available node for: '{}'", task.description);
                }
            }
        }

        // Simulate task completion (tasks complete after 1 tick of being InProgress)
        for task in tasks.iter_mut() {
            if task.status == TaskStatus::InProgress {
                println!("   🔄 IN PROGRESS: '{}'", task.description);
                // Mark as completed on next tick
                task.status = TaskStatus::Completed;
                
                // Free up the node
                if let Some(node_id) = task.assigned_to {
                    if let Some(node) = nodes.iter_mut().find(|n| n.id == node_id) {
                        node.current_task = None;
                        println!("   🎉 COMPLETED: '{}' by {}", task.description, node.name);
                    }
                }
            }
        }

        // Check if all tasks are done
        let all_done = tasks.iter().all(|t| t.status == TaskStatus::Completed);
        if all_done {
            break;
        }

        if tick > 10 {
            println!("\n⚠️  Max ticks reached, stopping simulation.");
            break;
        }

        std::thread::sleep(Duration::from_millis(500));
    }

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("✨ SWARM PLAN EXECUTION COMPLETE!\n");
    
    println!("📊 FINAL STATUS:");
    println!("───────────────────────────────────────────────────────────────");
    for task in &tasks {
        let status_icon = match task.status {
            TaskStatus::Completed => "✅",
            TaskStatus::InProgress => "🔄",
            TaskStatus::Pending => "⏳",
        };
        let assignee = task.assigned_to
            .and_then(|id| nodes.iter().find(|n| n.id == id))
            .map(|n| n.name.as_str())
            .unwrap_or("Unassigned");
        println!("  {} {} → {}", status_icon, task.description, assignee);
    }
    
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  The scheduler successfully matched tasks to nodes based     ║");
    println!("║  on skills and respected the dependency graph (DAG).         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}
