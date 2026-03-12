// =============================================================================
// STIGMERGY: Pheromone-Inspired Emergent Coordination
// =============================================================================
//
// Agents communicate indirectly through shared environment markers ("pheromones")
// that evaporate over time. This enables emergent coordination without central
// control, inspired by ant colony optimization.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AgentId;

/// A pheromone deposited by an agent into the shared environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pheromone {
    pub id: Uuid,
    pub depositor: AgentId,
    pub kind: PheromoneKind,
    pub location: PheromoneLocation,
    pub intensity: f64,
    pub metadata: HashMap<String, String>,
    pub deposited_at: DateTime<Utc>,
    pub half_life_secs: f64,
}

impl Pheromone {
    /// Current intensity after exponential decay.
    pub fn current_intensity(&self) -> f64 {
        let elapsed = Utc::now()
            .signed_duration_since(self.deposited_at)
            .num_milliseconds() as f64
            / 1000.0;
        self.intensity * (0.5_f64).powf(elapsed / self.half_life_secs)
    }

    /// Whether this pheromone has effectively evaporated.
    pub fn is_expired(&self, threshold: f64) -> bool {
        self.current_intensity() < threshold
    }
}

/// Semantic type of pheromone signal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PheromoneKind {
    /// "Good path found here" — attracts agents to productive routes.
    Trail,
    /// "Danger / avoid this" — repels agents from failed paths.
    Warning,
    /// "Resource available here" — marks discovered resources.
    Resource,
    /// "Task needs help" — recruits agents to a location.
    Recruitment,
    /// "I've already explored this" — prevents redundant work.
    Explored,
    /// "Consensus forming here" — marks decision convergence points.
    Consensus,
    /// Application-defined pheromone type.
    Custom(String),
}

/// Where in the abstract environment a pheromone is deposited.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PheromoneLocation {
    /// Domain or topic namespace (e.g., "web:crawl", "task:analysis").
    pub domain: String,
    /// Specific coordinate within the domain.
    pub coordinate: String,
}

/// Configuration for the stigmergy environment.
#[derive(Debug, Clone)]
pub struct StigmergyConfig {
    /// Minimum intensity before a pheromone is garbage-collected.
    pub evaporation_threshold: f64,
    /// Default half-life for new pheromones (seconds).
    pub default_half_life_secs: f64,
    /// Maximum pheromones per location before oldest are pruned.
    pub max_pheromones_per_location: usize,
    /// Maximum total pheromones in the environment.
    pub max_total_pheromones: usize,
    /// Intensity boost when multiple agents reinforce the same trail.
    pub reinforcement_factor: f64,
}

impl Default for StigmergyConfig {
    fn default() -> Self {
        Self {
            evaporation_threshold: 0.01,
            default_half_life_secs: 300.0,
            max_pheromones_per_location: 100,
            max_total_pheromones: 10_000,
            reinforcement_factor: 1.5,
        }
    }
}

/// The shared pheromone environment enabling indirect agent coordination.
pub struct StigmergyEnvironment {
    config: StigmergyConfig,
    /// Pheromones indexed by location.
    trails: DashMap<PheromoneLocation, Vec<Pheromone>>,
    /// Total pheromone count for capacity enforcement.
    total_count: std::sync::atomic::AtomicUsize,
}

impl StigmergyEnvironment {
    pub fn new(config: StigmergyConfig) -> Self {
        Self {
            config,
            trails: DashMap::new(),
            total_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Deposit a pheromone at a location.
    pub fn deposit(
        &self,
        depositor: AgentId,
        kind: PheromoneKind,
        location: PheromoneLocation,
        intensity: f64,
        metadata: HashMap<String, String>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let pheromone = Pheromone {
            id,
            depositor,
            kind: kind.clone(),
            location: location.clone(),
            intensity: intensity.clamp(0.0, 10.0),
            metadata,
            deposited_at: Utc::now(),
            half_life_secs: self.config.default_half_life_secs,
        };

        let mut entry = self.trails.entry(location).or_default();
        // Reinforce if same agent+kind already present at this location
        if let Some(existing) = entry.iter_mut().find(|p| {
            p.depositor == depositor && p.kind == kind && !p.is_expired(self.config.evaporation_threshold)
        }) {
            existing.intensity =
                (existing.intensity + intensity * self.config.reinforcement_factor).min(10.0);
            existing.deposited_at = Utc::now();
            return existing.id;
        }

        // Prune if at location capacity
        if entry.len() >= self.config.max_pheromones_per_location {
            entry.sort_by(|a, b| {
                a.current_intensity()
                    .partial_cmp(&b.current_intensity())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let removed = entry.remove(0);
            drop(removed);
            // total_count stays the same (remove + add)
        } else {
            self.total_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        entry.push(pheromone);
        id
    }

    /// Sense pheromones at a location, filtered by kind.
    pub fn sense(
        &self,
        location: &PheromoneLocation,
        kind_filter: Option<&PheromoneKind>,
    ) -> Vec<Pheromone> {
        let threshold = self.config.evaporation_threshold;
        self.trails
            .get(location)
            .map(|entry| {
                entry
                    .iter()
                    .filter(|p| !p.is_expired(threshold))
                    .filter(|p| kind_filter.is_none_or(|k| &p.kind == k))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Aggregate intensity at a location for a given pheromone kind.
    pub fn intensity_at(
        &self,
        location: &PheromoneLocation,
        kind: &PheromoneKind,
    ) -> f64 {
        self.sense(location, Some(kind))
            .iter()
            .map(|p| p.current_intensity())
            .sum()
    }

    /// Find the strongest signal among a set of candidate locations.
    pub fn strongest_signal(
        &self,
        locations: &[PheromoneLocation],
        kind: &PheromoneKind,
    ) -> Option<(PheromoneLocation, f64)> {
        locations
            .iter()
            .map(|loc| (loc.clone(), self.intensity_at(loc, kind)))
            .filter(|(_, intensity)| *intensity > self.config.evaporation_threshold)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Evaporate all expired pheromones (garbage collection).
    pub fn evaporate(&self) -> usize {
        let threshold = self.config.evaporation_threshold;
        let mut removed = 0usize;
        let mut empty_locations = Vec::new();

        for mut entry in self.trails.iter_mut() {
            let before = entry.value().len();
            entry.value_mut().retain(|p| !p.is_expired(threshold));
            removed += before - entry.value().len();
            if entry.value().is_empty() {
                empty_locations.push(entry.key().clone());
            }
        }

        for loc in empty_locations {
            self.trails.remove(&loc);
        }

        self.total_count
            .fetch_sub(removed, std::sync::atomic::Ordering::Relaxed);
        removed
    }

    /// Total active (non-expired) pheromone count.
    pub fn active_count(&self) -> usize {
        let threshold = self.config.evaporation_threshold;
        self.trails
            .iter()
            .map(|entry| {
                entry
                    .value()
                    .iter()
                    .filter(|p| !p.is_expired(threshold))
                    .count()
            })
            .sum()
    }

    /// Summary of pheromone distribution by kind.
    pub fn summary(&self) -> HashMap<PheromoneKind, usize> {
        let threshold = self.config.evaporation_threshold;
        let mut counts: HashMap<PheromoneKind, usize> = HashMap::new();
        for entry in self.trails.iter() {
            for p in entry.value().iter().filter(|p| !p.is_expired(threshold)) {
                *counts.entry(p.kind.clone()).or_default() += 1;
            }
        }
        counts
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(domain: &str, coord: &str) -> PheromoneLocation {
        PheromoneLocation {
            domain: domain.to_string(),
            coordinate: coord.to_string(),
        }
    }

    #[test]
    fn test_deposit_and_sense() {
        let env = StigmergyEnvironment::new(StigmergyConfig::default());
        let agent = AgentId::new();
        let location = loc("web", "example.com");

        env.deposit(agent, PheromoneKind::Trail, location.clone(), 1.0, HashMap::new());

        let sensed = env.sense(&location, None);
        assert_eq!(sensed.len(), 1);
        assert!(sensed[0].current_intensity() > 0.9);
    }

    #[test]
    fn test_pheromone_decay() {
        let p = Pheromone {
            id: Uuid::new_v4(),
            depositor: AgentId::new(),
            kind: PheromoneKind::Trail,
            location: loc("test", "a"),
            intensity: 1.0,
            metadata: HashMap::new(),
            deposited_at: Utc::now() - chrono::Duration::seconds(300),
            half_life_secs: 300.0,
        };
        // After one half-life, intensity should be ~0.5
        let intensity = p.current_intensity();
        assert!(intensity > 0.45 && intensity < 0.55, "got {intensity}");
    }

    #[test]
    fn test_expiration() {
        let p = Pheromone {
            id: Uuid::new_v4(),
            depositor: AgentId::new(),
            kind: PheromoneKind::Warning,
            location: loc("test", "b"),
            intensity: 0.1,
            metadata: HashMap::new(),
            deposited_at: Utc::now() - chrono::Duration::seconds(3000),
            half_life_secs: 300.0,
        };
        assert!(p.is_expired(0.01));
    }

    #[test]
    fn test_reinforcement() {
        let env = StigmergyEnvironment::new(StigmergyConfig::default());
        let agent = AgentId::new();
        let location = loc("web", "example.com");

        env.deposit(agent, PheromoneKind::Trail, location.clone(), 1.0, HashMap::new());
        // Same agent, same kind → reinforce
        env.deposit(agent, PheromoneKind::Trail, location.clone(), 1.0, HashMap::new());

        let sensed = env.sense(&location, Some(&PheromoneKind::Trail));
        assert_eq!(sensed.len(), 1);
        assert!(sensed[0].current_intensity() > 2.0, "reinforced intensity should be >2.0");
    }

    #[test]
    fn test_kind_filter() {
        let env = StigmergyEnvironment::new(StigmergyConfig::default());
        let agent = AgentId::new();
        let location = loc("web", "page1");

        env.deposit(agent, PheromoneKind::Trail, location.clone(), 1.0, HashMap::new());
        env.deposit(agent, PheromoneKind::Warning, location.clone(), 1.0, HashMap::new());

        assert_eq!(env.sense(&location, Some(&PheromoneKind::Trail)).len(), 1);
        assert_eq!(env.sense(&location, Some(&PheromoneKind::Warning)).len(), 1);
        assert_eq!(env.sense(&location, None).len(), 2);
    }

    #[test]
    fn test_strongest_signal() {
        let env = StigmergyEnvironment::new(StigmergyConfig::default());
        let agent = AgentId::new();

        let loc_a = loc("web", "a");
        let loc_b = loc("web", "b");
        let loc_c = loc("web", "c");

        env.deposit(agent, PheromoneKind::Trail, loc_a.clone(), 1.0, HashMap::new());
        env.deposit(agent, PheromoneKind::Trail, loc_b.clone(), 3.0, HashMap::new());
        env.deposit(agent, PheromoneKind::Trail, loc_c.clone(), 0.5, HashMap::new());

        let (best, intensity) = env
            .strongest_signal(&[loc_a, loc_b.clone(), loc_c], &PheromoneKind::Trail)
            .unwrap();
        assert_eq!(best, loc_b);
        assert!(intensity > 2.5);
    }

    #[test]
    fn test_evaporation_gc() {
        let config = StigmergyConfig {
            default_half_life_secs: 1.0,
            evaporation_threshold: 0.01,
            ..Default::default()
        };
        let env = StigmergyEnvironment::new(config);
        let agent = AgentId::new();
        let location = loc("test", "gc");

        // Manually insert an already-expired pheromone
        env.trails.entry(location.clone()).or_default().push(Pheromone {
            id: Uuid::new_v4(),
            depositor: agent,
            kind: PheromoneKind::Explored,
            location: location.clone(),
            intensity: 0.001,
            metadata: HashMap::new(),
            deposited_at: Utc::now() - chrono::Duration::seconds(3600),
            half_life_secs: 1.0,
        });

        assert_eq!(env.evaporate(), 1);
        assert_eq!(env.active_count(), 0);
    }

    #[test]
    fn test_summary() {
        let env = StigmergyEnvironment::new(StigmergyConfig::default());
        let a1 = AgentId::new();
        let a2 = AgentId::new();

        env.deposit(a1, PheromoneKind::Trail, loc("x", "1"), 1.0, HashMap::new());
        env.deposit(a2, PheromoneKind::Trail, loc("x", "2"), 1.0, HashMap::new());
        env.deposit(a1, PheromoneKind::Warning, loc("x", "1"), 1.0, HashMap::new());

        let s = env.summary();
        assert_eq!(s.get(&PheromoneKind::Trail), Some(&2));
        assert_eq!(s.get(&PheromoneKind::Warning), Some(&1));
    }

    #[test]
    fn test_location_capacity_pruning() {
        let config = StigmergyConfig {
            max_pheromones_per_location: 3,
            ..Default::default()
        };
        let env = StigmergyEnvironment::new(config);
        let location = loc("cap", "test");

        for i in 0..5 {
            let agent = AgentId::new();
            env.deposit(
                agent,
                PheromoneKind::Resource,
                location.clone(),
                (i + 1) as f64,
                HashMap::new(),
            );
        }

        let sensed = env.sense(&location, None);
        assert!(sensed.len() <= 3);
    }

    #[test]
    fn test_intensity_at() {
        let env = StigmergyEnvironment::new(StigmergyConfig::default());
        let location = loc("sum", "test");

        env.deposit(AgentId::new(), PheromoneKind::Trail, location.clone(), 2.0, HashMap::new());
        env.deposit(AgentId::new(), PheromoneKind::Trail, location.clone(), 3.0, HashMap::new());

        let total = env.intensity_at(&location, &PheromoneKind::Trail);
        assert!(total > 4.5, "aggregate intensity should be ~5.0, got {total}");
    }
}
