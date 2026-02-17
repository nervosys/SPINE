# Kubernetes Operator

The `spine-k8s` crate provides a Kubernetes operator for managing SPINE cluster deployments.

## SpineCluster CRD

```yaml
apiVersion: spine.nervosys.ai/v1
kind: SpineCluster
metadata:
  name: my-cluster
spec:
  replicas: 3
  image: ghcr.io/nervosys/spine:latest
  imagePullPolicy: IfNotPresent
  resources:
    cpu_request: "500m"
    cpu_limit: "2"
    memory_request: "256Mi"
    memory_limit: "1Gi"
  scaling:
    min_replicas: 2
    max_replicas: 10
    target_cpu_percent: 70
    target_memory_percent: 80
    max_connections_per_node: 1000
```

## Auto-Scaling

The operator monitors:
- **CPU utilization**: Scale when average exceeds target percentage
- **Memory utilization**: Scale when average exceeds target percentage
- **Connection count**: Scale when connections per node exceed threshold

Scaling decisions follow cooldown periods to prevent thrashing.

## Generated Manifests

The `ManifestGenerator` produces:
- **StatefulSet**: Ordered pod deployment with persistent identity
- **Service**: ClusterIP service for inter-pod communication
- **HPA**: Horizontal Pod Autoscaler with CPU/memory targets

## Health Monitoring

Periodic health checks against each pod's `/health` endpoint with configurable:
- Check interval
- Failure threshold before pod restart
- Success threshold for recovery
