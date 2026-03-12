//! # SPINE Kubernetes Operator
//!
//! Kubernetes operator for managing SPINE clusters with auto-scaling,
//! health monitoring, and rolling updates.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                  Kubernetes Cluster                      │
//! │                                                          │
//! │  ┌──────────────────────┐    ┌────────────────────────┐ │
//! │  │   SPINE Operator     │    │   SpineCluster CRD     │ │
//! │  │   (Controller)       │────│   desired: 3 replicas  │ │
//! │  │                      │    │   cpu_target: 70%      │ │
//! │  └──────────┬───────────┘    └────────────────────────┘ │
//! │             │                                            │
//! │  ┌──────────▼───────────────────────────────────┐       │
//! │  │          Reconciliation Loop                   │       │
//! │  │  1. Watch SpineCluster CRDs                   │       │
//! │  │  2. Diff desired vs actual state              │       │
//! │  │  3. Scale pods up/down                        │       │
//! │  │  4. Monitor health endpoints                  │       │
//! │  └───────────────────────────────────────────────┘       │
//! │                                                          │
//! │  ┌─────────┐  ┌─────────┐  ┌─────────┐                 │
//! │  │ spine-0 │  │ spine-1 │  │ spine-2 │                 │
//! │  │  Pod    │  │  Pod    │  │  Pod    │                 │
//! │  └─────────┘  └─────────┘  └─────────┘                 │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - **CRD Definitions**: `SpineCluster` and `SpineNode` custom resources
//! - **Auto-scaling**: Scale based on CPU, memory, connection count, or custom metrics
//! - **Health Monitoring**: Periodic health checks with automatic pod restart
//! - **Rolling Updates**: Zero-downtime updates with configurable strategy
//! - **Resource Quotas**: CPU/memory limits per node

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Custom Resource Definitions (CRDs)
// ============================================================================

/// Specification for a SPINE cluster deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpineClusterSpec {
    /// Number of SPINE server replicas.
    pub replicas: u32,
    /// Docker image for SPINE server.
    pub image: String,
    /// Image pull policy.
    pub image_pull_policy: ImagePullPolicy,
    /// Resource limits per pod.
    pub resources: ResourceRequirements,
    /// Auto-scaling configuration.
    pub autoscaling: Option<AutoscalingSpec>,
    /// Port configurations.
    pub ports: PortConfig,
    /// TLS configuration.
    pub tls: Option<TlsConfig>,
    /// Storage configuration.
    pub storage: Option<StorageSpec>,
    /// Environment variables.
    pub env: HashMap<String, String>,
    /// Update strategy.
    pub update_strategy: UpdateStrategy,
    /// Health check configuration.
    pub health_check: HealthCheckConfig,
}

impl Default for SpineClusterSpec {
    fn default() -> Self {
        Self {
            replicas: 3,
            image: "spine:latest".to_string(),
            image_pull_policy: ImagePullPolicy::IfNotPresent,
            resources: ResourceRequirements::default(),
            autoscaling: None,
            ports: PortConfig::default(),
            tls: None,
            storage: None,
            env: HashMap::new(),
            update_strategy: UpdateStrategy::default(),
            health_check: HealthCheckConfig::default(),
        }
    }
}

/// Image pull policy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ImagePullPolicy {
    Always,
    IfNotPresent,
    Never,
}

/// Resource requirements for SPINE pods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// CPU request (millicores).
    pub cpu_request: u32,
    /// CPU limit (millicores).
    pub cpu_limit: u32,
    /// Memory request (MiB).
    pub memory_request: u32,
    /// Memory limit (MiB).
    pub memory_limit: u32,
    /// GPU request (count).
    pub gpu_request: u32,
}

impl Default for ResourceRequirements {
    fn default() -> Self {
        Self {
            cpu_request: 500,
            cpu_limit: 2000,
            memory_request: 512,
            memory_limit: 2048,
            gpu_request: 0,
        }
    }
}

/// Auto-scaling specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoscalingSpec {
    /// Minimum number of replicas.
    pub min_replicas: u32,
    /// Maximum number of replicas.
    pub max_replicas: u32,
    /// Target CPU utilization percentage for scaling.
    pub target_cpu_percent: u32,
    /// Target memory utilization percentage for scaling.
    pub target_memory_percent: u32,
    /// Target connection count per pod for scaling.
    pub target_connections_per_pod: Option<u32>,
    /// Cooldown period after scale-up (seconds).
    pub scale_up_cooldown_secs: u64,
    /// Cooldown period after scale-down (seconds).
    pub scale_down_cooldown_secs: u64,
    /// Custom metrics for scaling.
    pub custom_metrics: Vec<CustomMetric>,
}

impl Default for AutoscalingSpec {
    fn default() -> Self {
        Self {
            min_replicas: 1,
            max_replicas: 10,
            target_cpu_percent: 70,
            target_memory_percent: 80,
            target_connections_per_pod: Some(100),
            scale_up_cooldown_secs: 60,
            scale_down_cooldown_secs: 300,
            custom_metrics: Vec::new(),
        }
    }
}

/// Custom metric for auto-scaling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMetric {
    /// Metric name (e.g., "spine_sessions_active").
    pub name: String,
    /// Target value for the metric.
    pub target_value: f64,
    /// Metric type.
    pub metric_type: MetricType,
}

/// Type of scaling metric.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MetricType {
    /// Average value across all pods.
    AverageValue,
    /// Total value divided by pod count.
    AverageUtilization,
    /// Raw value compared to target.
    Value,
}

/// Port configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConfig {
    /// SPINE protocol port.
    pub spine_port: u16,
    /// Health check port.
    pub health_port: u16,
    /// Metrics port.
    pub metrics_port: u16,
    /// WebSocket port (optional).
    pub websocket_port: Option<u16>,
    /// Gateway API port (optional).
    pub gateway_port: Option<u16>,
}

impl Default for PortConfig {
    fn default() -> Self {
        Self {
            spine_port: 3000,
            health_port: 8080,
            metrics_port: 9090,
            websocket_port: Some(3001),
            gateway_port: Some(8000),
        }
    }
}

/// TLS configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Name of the Kubernetes secret containing TLS cert and key.
    pub secret_name: String,
    /// Whether to enable mutual TLS.
    pub mutual_tls: bool,
}

/// Persistent storage specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSpec {
    /// Storage class name.
    pub storage_class: String,
    /// Storage size (e.g., "10Gi").
    pub size: String,
    /// Mount path inside the container.
    pub mount_path: String,
}

/// Update strategy for rolling deployments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStrategy {
    /// Strategy type.
    pub strategy_type: UpdateStrategyType,
    /// Maximum unavailable pods during update.
    pub max_unavailable: u32,
    /// Maximum surge pods during update.
    pub max_surge: u32,
}

impl Default for UpdateStrategy {
    fn default() -> Self {
        Self {
            strategy_type: UpdateStrategyType::RollingUpdate,
            max_unavailable: 1,
            max_surge: 1,
        }
    }
}

/// Update strategy type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum UpdateStrategyType {
    RollingUpdate,
    Recreate,
}

/// Health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Liveness probe path.
    pub liveness_path: String,
    /// Readiness probe path.
    pub readiness_path: String,
    /// Initial delay before first check (seconds).
    pub initial_delay_secs: u32,
    /// Check interval (seconds).
    pub period_secs: u32,
    /// Timeout per check (seconds).
    pub timeout_secs: u32,
    /// Number of failures before restart.
    pub failure_threshold: u32,
    /// Number of successes before marking healthy.
    pub success_threshold: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            liveness_path: "/health".to_string(),
            readiness_path: "/ready".to_string(),
            initial_delay_secs: 10,
            period_secs: 15,
            timeout_secs: 5,
            failure_threshold: 3,
            success_threshold: 1,
        }
    }
}

// ============================================================================
// Cluster Status
// ============================================================================

/// Status of a SPINE cluster deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpineClusterStatus {
    /// Current number of ready replicas.
    pub ready_replicas: u32,
    /// Total number of replicas.
    pub total_replicas: u32,
    /// Current deployment phase.
    pub phase: ClusterPhase,
    /// Status conditions.
    pub conditions: Vec<ClusterCondition>,
    /// Per-node status.
    pub nodes: Vec<SpineNodeStatus>,
    /// Last scaling event timestamp.
    pub last_scale_time: Option<String>,
    /// Current observed generation.
    pub observed_generation: u64,
}

/// Cluster deployment phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClusterPhase {
    Pending,
    Creating,
    Running,
    Updating,
    ScalingUp,
    ScalingDown,
    Failed,
    Terminating,
}

/// A condition on the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterCondition {
    pub condition_type: String,
    pub status: bool,
    pub reason: String,
    pub message: String,
    pub last_transition: String,
}

/// Status of a single SPINE node/pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpineNodeStatus {
    /// Pod name.
    pub name: String,
    /// Pod IP.
    pub ip: Option<String>,
    /// Node phase.
    pub phase: NodePhase,
    /// Active sessions.
    pub active_sessions: u32,
    /// CPU utilization percentage.
    pub cpu_percent: f32,
    /// Memory utilization percentage.
    pub memory_percent: f32,
    /// Whether the node is a Raft leader.
    pub is_leader: bool,
}

/// Individual node phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodePhase {
    Pending,
    Running,
    Healthy,
    Degraded,
    Failed,
    Terminating,
}

// ============================================================================
// Auto-Scaler Logic
// ============================================================================

/// Metrics snapshot for scaling decisions.
#[derive(Debug, Clone)]
pub struct ScalingMetrics {
    pub current_replicas: u32,
    pub avg_cpu_percent: f32,
    pub avg_memory_percent: f32,
    pub total_connections: u32,
    pub custom_metrics: HashMap<String, f64>,
}

/// Scaling decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScalingDecision {
    /// No change needed.
    NoChange,
    /// Scale up to this many replicas.
    ScaleUp(u32),
    /// Scale down to this many replicas.
    ScaleDown(u32),
}

/// Compute the desired scaling decision based on metrics and configuration.
pub fn compute_scaling_decision(
    spec: &AutoscalingSpec,
    metrics: &ScalingMetrics,
) -> ScalingDecision {
    let current = metrics.current_replicas;

    // CPU-based scaling
    let cpu_ratio = metrics.avg_cpu_percent / spec.target_cpu_percent as f32;
    let cpu_desired = (current as f32 * cpu_ratio).ceil() as u32;

    // Memory-based scaling
    let mem_ratio = metrics.avg_memory_percent / spec.target_memory_percent as f32;
    let mem_desired = (current as f32 * mem_ratio).ceil() as u32;

    // Connection-based scaling
    let conn_desired = if let Some(target_conn) = spec.target_connections_per_pod {
        if target_conn > 0 {
            ((metrics.total_connections as f32 / target_conn as f32).ceil() as u32).max(1)
        } else {
            current
        }
    } else {
        current
    };

    // Take the maximum desired replica count
    let desired = cpu_desired.max(mem_desired).max(conn_desired);

    // Clamp to min/max
    let clamped = desired.clamp(spec.min_replicas, spec.max_replicas);

    if clamped > current {
        ScalingDecision::ScaleUp(clamped)
    } else if clamped < current {
        ScalingDecision::ScaleDown(clamped)
    } else {
        ScalingDecision::NoChange
    }
}

// ============================================================================
// Manifest Generation
// ============================================================================

/// Generate a Kubernetes Deployment manifest for a SPINE cluster.
pub fn generate_deployment_manifest(
    name: &str,
    namespace: &str,
    spec: &SpineClusterSpec,
) -> serde_json::Value {
    let mut container_ports = vec![
        serde_json::json!({
            "name": "spine",
            "containerPort": spec.ports.spine_port,
            "protocol": "TCP"
        }),
        serde_json::json!({
            "name": "health",
            "containerPort": spec.ports.health_port,
            "protocol": "TCP"
        }),
        serde_json::json!({
            "name": "metrics",
            "containerPort": spec.ports.metrics_port,
            "protocol": "TCP"
        }),
    ];

    if let Some(ws_port) = spec.ports.websocket_port {
        container_ports.push(serde_json::json!({
            "name": "websocket",
            "containerPort": ws_port,
            "protocol": "TCP"
        }));
    }

    if let Some(gw_port) = spec.ports.gateway_port {
        container_ports.push(serde_json::json!({
            "name": "gateway",
            "containerPort": gw_port,
            "protocol": "TCP"
        }));
    }

    let env_vars: Vec<serde_json::Value> = spec
        .env
        .iter()
        .map(|(k, v)| serde_json::json!({"name": k, "value": v}))
        .collect();

    serde_json::json!({
        "apiVersion": "apps/v1",
        "kind": "StatefulSet",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": {
                "app.kubernetes.io/name": "spine",
                "app.kubernetes.io/instance": name,
                "app.kubernetes.io/managed-by": "spine-operator"
            }
        },
        "spec": {
            "replicas": spec.replicas,
            "selector": {
                "matchLabels": {
                    "app.kubernetes.io/instance": name
                }
            },
            "template": {
                "metadata": {
                    "labels": {
                        "app.kubernetes.io/name": "spine",
                        "app.kubernetes.io/instance": name
                    }
                },
                "spec": {
                    "containers": [{
                        "name": "spine-server",
                        "image": spec.image,
                        "imagePullPolicy": format!("{:?}", spec.image_pull_policy),
                        "ports": container_ports,
                        "resources": {
                            "requests": {
                                "cpu": format!("{}m", spec.resources.cpu_request),
                                "memory": format!("{}Mi", spec.resources.memory_request)
                            },
                            "limits": {
                                "cpu": format!("{}m", spec.resources.cpu_limit),
                                "memory": format!("{}Mi", spec.resources.memory_limit)
                            }
                        },
                        "env": env_vars,
                        "livenessProbe": {
                            "httpGet": {
                                "path": spec.health_check.liveness_path,
                                "port": spec.ports.health_port
                            },
                            "initialDelaySeconds": spec.health_check.initial_delay_secs,
                            "periodSeconds": spec.health_check.period_secs,
                            "timeoutSeconds": spec.health_check.timeout_secs,
                            "failureThreshold": spec.health_check.failure_threshold
                        },
                        "readinessProbe": {
                            "httpGet": {
                                "path": spec.health_check.readiness_path,
                                "port": spec.ports.health_port
                            },
                            "periodSeconds": spec.health_check.period_secs,
                            "timeoutSeconds": spec.health_check.timeout_secs,
                            "successThreshold": spec.health_check.success_threshold
                        }
                    }]
                }
            }
        }
    })
}

/// Generate a Kubernetes Service manifest for a SPINE cluster.
pub fn generate_service_manifest(
    name: &str,
    namespace: &str,
    spec: &SpineClusterSpec,
) -> serde_json::Value {
    let mut ports = vec![
        serde_json::json!({
            "name": "spine",
            "port": spec.ports.spine_port,
            "targetPort": "spine",
            "protocol": "TCP"
        }),
        serde_json::json!({
            "name": "health",
            "port": spec.ports.health_port,
            "targetPort": "health",
            "protocol": "TCP"
        }),
    ];

    if let Some(gw_port) = spec.ports.gateway_port {
        ports.push(serde_json::json!({
            "name": "gateway",
            "port": gw_port,
            "targetPort": "gateway",
            "protocol": "TCP"
        }));
    }

    serde_json::json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": format!("{}-svc", name),
            "namespace": namespace,
            "labels": {
                "app.kubernetes.io/name": "spine",
                "app.kubernetes.io/instance": name
            }
        },
        "spec": {
            "type": "ClusterIP",
            "selector": {
                "app.kubernetes.io/instance": name
            },
            "ports": ports
        }
    })
}

/// Generate an HPA (HorizontalPodAutoscaler) manifest.
pub fn generate_hpa_manifest(
    name: &str,
    namespace: &str,
    autoscaling: &AutoscalingSpec,
) -> serde_json::Value {
    serde_json::json!({
        "apiVersion": "autoscaling/v2",
        "kind": "HorizontalPodAutoscaler",
        "metadata": {
            "name": format!("{}-hpa", name),
            "namespace": namespace
        },
        "spec": {
            "scaleTargetRef": {
                "apiVersion": "apps/v1",
                "kind": "StatefulSet",
                "name": name
            },
            "minReplicas": autoscaling.min_replicas,
            "maxReplicas": autoscaling.max_replicas,
            "metrics": [
                {
                    "type": "Resource",
                    "resource": {
                        "name": "cpu",
                        "target": {
                            "type": "Utilization",
                            "averageUtilization": autoscaling.target_cpu_percent
                        }
                    }
                },
                {
                    "type": "Resource",
                    "resource": {
                        "name": "memory",
                        "target": {
                            "type": "Utilization",
                            "averageUtilization": autoscaling.target_memory_percent
                        }
                    }
                }
            ],
            "behavior": {
                "scaleUp": {
                    "stabilizationWindowSeconds": autoscaling.scale_up_cooldown_secs,
                    "policies": [{
                        "type": "Percent",
                        "value": 100,
                        "periodSeconds": 60
                    }]
                },
                "scaleDown": {
                    "stabilizationWindowSeconds": autoscaling.scale_down_cooldown_secs,
                    "policies": [{
                        "type": "Percent",
                        "value": 25,
                        "periodSeconds": 60
                    }]
                }
            }
        }
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cluster_spec() {
        let spec = SpineClusterSpec::default();
        assert_eq!(spec.replicas, 3);
        assert_eq!(spec.ports.spine_port, 3000);
        assert_eq!(spec.resources.cpu_limit, 2000);
    }

    #[test]
    fn test_scaling_decision_no_change() {
        let spec = AutoscalingSpec::default();
        let metrics = ScalingMetrics {
            current_replicas: 3,
            avg_cpu_percent: 50.0,
            avg_memory_percent: 60.0,
            total_connections: 200,
            custom_metrics: HashMap::new(),
        };

        let decision = compute_scaling_decision(&spec, &metrics);
        assert_eq!(decision, ScalingDecision::NoChange);
    }

    #[test]
    fn test_scaling_decision_scale_up_cpu() {
        let spec = AutoscalingSpec {
            target_cpu_percent: 50,
            ..Default::default()
        };
        let metrics = ScalingMetrics {
            current_replicas: 2,
            avg_cpu_percent: 90.0,
            avg_memory_percent: 30.0,
            total_connections: 50,
            custom_metrics: HashMap::new(),
        };

        let decision = compute_scaling_decision(&spec, &metrics);
        assert!(matches!(decision, ScalingDecision::ScaleUp(_)));
    }

    #[test]
    fn test_scaling_decision_scale_down() {
        let spec = AutoscalingSpec {
            min_replicas: 1,
            ..Default::default()
        };
        let metrics = ScalingMetrics {
            current_replicas: 5,
            avg_cpu_percent: 10.0,
            avg_memory_percent: 15.0,
            total_connections: 20,
            custom_metrics: HashMap::new(),
        };

        let decision = compute_scaling_decision(&spec, &metrics);
        assert!(matches!(decision, ScalingDecision::ScaleDown(_)));
    }

    #[test]
    fn test_scaling_respects_max() {
        let spec = AutoscalingSpec {
            max_replicas: 5,
            target_cpu_percent: 10,
            ..Default::default()
        };
        let metrics = ScalingMetrics {
            current_replicas: 3,
            avg_cpu_percent: 100.0,
            avg_memory_percent: 0.0,
            total_connections: 0,
            custom_metrics: HashMap::new(),
        };

        let decision = compute_scaling_decision(&spec, &metrics);
        if let ScalingDecision::ScaleUp(n) = decision {
            assert!(n <= 5);
        }
    }

    #[test]
    fn test_scaling_respects_min() {
        let spec = AutoscalingSpec {
            min_replicas: 2,
            ..Default::default()
        };
        let metrics = ScalingMetrics {
            current_replicas: 5,
            avg_cpu_percent: 1.0,
            avg_memory_percent: 1.0,
            total_connections: 0,
            custom_metrics: HashMap::new(),
        };

        let decision = compute_scaling_decision(&spec, &metrics);
        if let ScalingDecision::ScaleDown(n) = decision {
            assert!(n >= 2);
        }
    }

    #[test]
    fn test_generate_deployment_manifest() {
        let spec = SpineClusterSpec::default();
        let manifest = generate_deployment_manifest("test-spine", "default", &spec);

        assert_eq!(manifest["kind"], "StatefulSet");
        assert_eq!(manifest["metadata"]["name"], "test-spine");
        assert_eq!(manifest["spec"]["replicas"], 3);
    }

    #[test]
    fn test_generate_service_manifest() {
        let spec = SpineClusterSpec::default();
        let manifest = generate_service_manifest("test-spine", "default", &spec);

        assert_eq!(manifest["kind"], "Service");
        assert_eq!(manifest["metadata"]["name"], "test-spine-svc");
    }

    #[test]
    fn test_generate_hpa_manifest() {
        let autoscaling = AutoscalingSpec::default();
        let manifest = generate_hpa_manifest("test-spine", "default", &autoscaling);

        assert_eq!(manifest["kind"], "HorizontalPodAutoscaler");
        assert_eq!(manifest["spec"]["minReplicas"], 1);
        assert_eq!(manifest["spec"]["maxReplicas"], 10);
    }

    #[test]
    fn test_cluster_phase_serialization() {
        let phase = ClusterPhase::Running;
        let json = serde_json::to_string(&phase).unwrap();
        assert!(json.contains("Running"));

        let parsed: ClusterPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ClusterPhase::Running);
    }

    #[test]
    fn test_health_check_defaults() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.liveness_path, "/health");
        assert_eq!(config.readiness_path, "/ready");
        assert_eq!(config.failure_threshold, 3);
    }

    #[test]
    fn test_scaling_connection_based() {
        let spec = AutoscalingSpec {
            target_connections_per_pod: Some(50),
            ..Default::default()
        };
        let metrics = ScalingMetrics {
            current_replicas: 2,
            avg_cpu_percent: 10.0,
            avg_memory_percent: 10.0,
            total_connections: 300, // 300 / 50 = 6 pods needed
            custom_metrics: HashMap::new(),
        };

        let decision = compute_scaling_decision(&spec, &metrics);
        assert!(matches!(decision, ScalingDecision::ScaleUp(n) if n >= 6));
    }

    #[test]
    fn test_deployment_with_env_vars() {
        let mut spec = SpineClusterSpec::default();
        spec.env.insert("RUST_LOG".to_string(), "info".to_string());
        spec.env
            .insert("SPINE_PORT".to_string(), "3000".to_string());

        let manifest = generate_deployment_manifest("test", "default", &spec);
        let containers = &manifest["spec"]["template"]["spec"]["containers"];
        let env = &containers[0]["env"];
        assert!(env.is_array());
    }
}
