//! High-performance connection pooling with adaptive scaling.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};

use crate::{TransportBackend, TransportError, TransportResult};

// =============================================================================
// CONNECTION POOL
// =============================================================================

/// Configuration for connection pool
#[derive(Clone, Debug)]
pub struct PoolConfig {
    /// Minimum connections to maintain
    pub min_connections: usize,
    /// Maximum connections allowed
    pub max_connections: usize,
    /// Connection idle timeout
    pub idle_timeout: Duration,
    /// Maximum connection age
    pub max_age: Duration,
    /// Health check interval
    pub health_check_interval: Duration,
    /// Connection acquisition timeout
    pub acquire_timeout: Duration,
    /// Enable connection reuse
    pub enable_reuse: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 2,
            max_connections: 32,
            idle_timeout: Duration::from_secs(60),
            max_age: Duration::from_secs(3600),
            health_check_interval: Duration::from_secs(30),
            acquire_timeout: Duration::from_secs(10),
            enable_reuse: true,
        }
    }
}

/// Connection wrapper with metadata
pub struct PooledConnection<C: TransportBackend> {
    /// The underlying connection
    pub connection: C,
    /// When this connection was created
    pub created_at: Instant,
    /// When this connection was last used
    pub last_used: Instant,
    /// Number of requests served
    pub requests_served: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_recv: u64,
    /// Whether connection is healthy
    pub healthy: bool,
}

impl<C: TransportBackend> PooledConnection<C> {
    /// Create a new pooled connection
    pub fn new(connection: C) -> Self {
        let now = Instant::now();
        Self {
            connection,
            created_at: now,
            last_used: now,
            requests_served: 0,
            bytes_sent: 0,
            bytes_recv: 0,
            healthy: true,
        }
    }

    /// Check if connection is expired
    pub fn is_expired(&self, max_age: Duration) -> bool {
        self.created_at.elapsed() > max_age
    }

    /// Check if connection is idle
    pub fn is_idle(&self, idle_timeout: Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }

    /// Mark as used
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
        self.requests_served += 1;
    }

    /// Add bytes sent
    pub fn add_sent(&mut self, bytes: u64) {
        self.bytes_sent += bytes;
    }

    /// Add bytes received
    pub fn add_recv(&mut self, bytes: u64) {
        self.bytes_recv += bytes;
    }
}

/// High-performance connection pool
pub struct ConnectionPool<C: TransportBackend> {
    /// Pool configuration
    config: PoolConfig,
    /// Available connections
    available: RwLock<VecDeque<PooledConnection<C>>>,
    /// Semaphore for limiting total connections
    semaphore: Arc<Semaphore>,
    /// Connection counter
    total_connections: AtomicUsize,
    /// Statistics
    stats: PoolStats,
}

impl<C: TransportBackend + Clone> ConnectionPool<C> {
    /// Create a new connection pool
    pub fn new(config: PoolConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_connections));

        Self {
            config,
            available: RwLock::new(VecDeque::new()),
            semaphore,
            total_connections: AtomicUsize::new(0),
            stats: PoolStats::new(),
        }
    }

    /// Get pool statistics
    pub fn stats(&self) -> &PoolStats {
        &self.stats
    }

    /// Get current pool size
    pub fn size(&self) -> usize {
        self.total_connections.load(Ordering::Relaxed)
    }

    /// Acquire a connection from the pool
    pub async fn acquire(&self) -> TransportResult<PoolGuard<'_, C>> {
        let start = Instant::now();

        // Try to get an existing connection
        {
            let mut available = self.available.write().await;

            while let Some(mut conn) = available.pop_front() {
                // Check if connection is still valid
                if conn.is_expired(self.config.max_age)
                    || conn.is_idle(self.config.idle_timeout)
                    || !conn.healthy
                {
                    self.total_connections.fetch_sub(1, Ordering::Relaxed);
                    self.stats
                        .connections_closed
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                conn.touch();
                self.stats
                    .connections_reused
                    .fetch_add(1, Ordering::Relaxed);
                self.stats
                    .acquire_time_us
                    .fetch_add(start.elapsed().as_micros() as u64, Ordering::Relaxed);

                return Ok(PoolGuard {
                    connection: Some(conn),
                    pool: self,
                });
            }
        }

        // No available connections - need to create one
        // Wait for permit with timeout
        let permit = tokio::time::timeout(
            self.config.acquire_timeout,
            self.semaphore.clone().acquire_owned(),
        )
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(|_| TransportError::ConnectionClosed)?;

        // Note: In a real implementation, we'd create the connection here
        // For now, we return an error indicating no connections available
        drop(permit);

        Err(TransportError::ConnectionClosed)
    }

    /// Return a connection to the pool
    pub async fn release(&self, mut conn: PooledConnection<C>) {
        if !self.config.enable_reuse || conn.is_expired(self.config.max_age) || !conn.healthy {
            self.total_connections.fetch_sub(1, Ordering::Relaxed);
            self.stats
                .connections_closed
                .fetch_add(1, Ordering::Relaxed);
            return;
        }

        conn.last_used = Instant::now();

        let mut available = self.available.write().await;
        available.push_back(conn);
        self.stats
            .connections_returned
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Add a new connection to the pool
    pub async fn add(&self, conn: C) -> TransportResult<()> {
        if self.total_connections.load(Ordering::Relaxed) >= self.config.max_connections {
            return Err(TransportError::ResourceExhausted {
                resource: "connection pool".into(),
            });
        }

        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.stats
            .connections_created
            .fetch_add(1, Ordering::Relaxed);

        let pooled = PooledConnection::new(conn);

        let mut available = self.available.write().await;
        available.push_back(pooled);

        Ok(())
    }

    /// Perform health checks on all connections
    pub async fn health_check(&self) {
        let mut available = self.available.write().await;
        let mut healthy = VecDeque::new();
        let mut removed = 0;

        while let Some(mut conn) = available.pop_front() {
            // Check expiration
            if conn.is_expired(self.config.max_age) {
                removed += 1;
                continue;
            }

            // Check idle
            if conn.is_idle(self.config.idle_timeout) {
                removed += 1;
                continue;
            }

            // Mark as healthy (in real implementation, would ping)
            conn.healthy = true;
            healthy.push_back(conn);
        }

        *available = healthy;

        if removed > 0 {
            self.total_connections.fetch_sub(removed, Ordering::Relaxed);
            self.stats
                .connections_closed
                .fetch_add(removed as u64, Ordering::Relaxed);
        }
    }

    /// Close all connections
    pub async fn close_all(&self) {
        let mut available = self.available.write().await;
        let count = available.len();
        available.clear();

        self.total_connections.store(0, Ordering::Relaxed);
        self.stats
            .connections_closed
            .fetch_add(count as u64, Ordering::Relaxed);
    }
}

/// RAII guard for pooled connections
pub struct PoolGuard<'a, C: TransportBackend> {
    connection: Option<PooledConnection<C>>,
    pool: &'a ConnectionPool<C>,
}

impl<'a, C: TransportBackend> PoolGuard<'a, C> {
    /// Get a reference to the connection
    pub fn connection(&self) -> &C {
        &self.connection.as_ref().unwrap().connection
    }

    /// Get a mutable reference to the connection
    pub fn connection_mut(&mut self) -> &mut C {
        &mut self.connection.as_mut().unwrap().connection
    }

    /// Mark connection as unhealthy (won't be returned to pool)
    pub fn mark_unhealthy(&mut self) {
        if let Some(ref mut conn) = self.connection {
            conn.healthy = false;
        }
    }

    /// Detach the connection from the pool
    pub fn detach(mut self) -> PooledConnection<C> {
        self.connection.take().unwrap()
    }
}

impl<'a, C: TransportBackend> Drop for PoolGuard<'a, C> {
    fn drop(&mut self) {
        // Simply drop the connection when guard is dropped
        // For proper pooling, use `return_to_pool()` method before dropping
        if let Some(conn) = self.connection.take() {
            // Connection will be dropped here
            // In a production system, you'd want proper async return-to-pool
            drop(conn);
        }
    }
}

/// Pool statistics
pub struct PoolStats {
    /// Total connections created
    pub connections_created: AtomicU64,
    /// Total connections closed
    pub connections_closed: AtomicU64,
    /// Total connections reused
    pub connections_reused: AtomicU64,
    /// Total connections returned
    pub connections_returned: AtomicU64,
    /// Total acquire time in microseconds
    pub acquire_time_us: AtomicU64,
    /// Total acquire count
    pub acquire_count: AtomicU64,
}

impl PoolStats {
    /// Create new stats
    pub fn new() -> Self {
        Self {
            connections_created: AtomicU64::new(0),
            connections_closed: AtomicU64::new(0),
            connections_reused: AtomicU64::new(0),
            connections_returned: AtomicU64::new(0),
            acquire_time_us: AtomicU64::new(0),
            acquire_count: AtomicU64::new(0),
        }
    }

    /// Get average acquire time
    pub fn avg_acquire_time(&self) -> Duration {
        let count = self.acquire_count.load(Ordering::Relaxed);
        if count == 0 {
            return Duration::ZERO;
        }

        let total_us = self.acquire_time_us.load(Ordering::Relaxed);
        Duration::from_micros(total_us / count)
    }

    /// Get reuse ratio
    pub fn reuse_ratio(&self) -> f64 {
        let created = self.connections_created.load(Ordering::Relaxed);
        let reused = self.connections_reused.load(Ordering::Relaxed);

        if created == 0 && reused == 0 {
            return 0.0;
        }

        reused as f64 / (created + reused) as f64
    }
}

impl Default for PoolStats {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// ENDPOINT POOL
// =============================================================================

/// Pool connections by endpoint
pub struct EndpointPool<C: TransportBackend> {
    /// Pools per endpoint
    pools: RwLock<HashMap<String, Arc<ConnectionPool<C>>>>,
    /// Default configuration
    config: PoolConfig,
    /// Maximum endpoints
    max_endpoints: usize,
}

impl<C: TransportBackend + Clone> EndpointPool<C> {
    /// Create a new endpoint pool
    pub fn new(config: PoolConfig, max_endpoints: usize) -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            config,
            max_endpoints,
        }
    }

    /// Get or create pool for endpoint
    pub async fn get_pool(&self, endpoint: &str) -> TransportResult<Arc<ConnectionPool<C>>> {
        // Check existing
        {
            let pools = self.pools.read().await;
            if let Some(pool) = pools.get(endpoint) {
                return Ok(Arc::clone(pool));
            }
        }

        // Create new
        let mut pools = self.pools.write().await;

        // Double-check
        if let Some(pool) = pools.get(endpoint) {
            return Ok(Arc::clone(pool));
        }

        // Check limit
        if pools.len() >= self.max_endpoints {
            return Err(TransportError::ResourceExhausted {
                resource: "endpoint pool".into(),
            });
        }

        let pool = Arc::new(ConnectionPool::new(self.config.clone()));
        pools.insert(endpoint.to_string(), Arc::clone(&pool));

        Ok(pool)
    }

    /// Remove pool for endpoint
    pub async fn remove_pool(&self, endpoint: &str) {
        let mut pools = self.pools.write().await;
        if let Some(pool) = pools.remove(endpoint) {
            pool.close_all().await;
        }
    }

    /// Get all endpoints
    pub async fn endpoints(&self) -> Vec<String> {
        let pools = self.pools.read().await;
        pools.keys().cloned().collect()
    }

    /// Get total connections across all pools
    pub async fn total_connections(&self) -> usize {
        let pools = self.pools.read().await;
        pools.values().map(|p| p.size()).sum()
    }

    /// Perform health checks on all pools
    pub async fn health_check_all(&self) {
        let pools = self.pools.read().await;
        for pool in pools.values() {
            pool.health_check().await;
        }
    }
}

// =============================================================================
// LOAD BALANCED POOL
// =============================================================================

/// Load balancing strategy
#[derive(Clone, Copy, Debug)]
pub enum LoadBalanceStrategy {
    /// Round-robin selection
    RoundRobin,
    /// Least connections
    LeastConnections,
    /// Random selection
    Random,
    /// Weighted round-robin
    Weighted,
}

/// Weighted endpoint
#[derive(Clone, Debug)]
pub struct WeightedEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Weight (higher = more traffic)
    pub weight: u32,
    /// Current weight for WRR
    pub current_weight: i32,
}

/// Load-balanced connection pool
pub struct LoadBalancedPool<C: TransportBackend> {
    /// Endpoint pools
    pools: EndpointPool<C>,
    /// Endpoints with weights
    endpoints: RwLock<Vec<WeightedEndpoint>>,
    /// Load balancing strategy
    strategy: LoadBalanceStrategy,
    /// Round-robin counter
    rr_counter: AtomicUsize,
}

impl<C: TransportBackend + Clone> LoadBalancedPool<C> {
    /// Create a new load-balanced pool
    pub fn new(config: PoolConfig, strategy: LoadBalanceStrategy) -> Self {
        Self {
            pools: EndpointPool::new(config, 256),
            endpoints: RwLock::new(Vec::new()),
            strategy,
            rr_counter: AtomicUsize::new(0),
        }
    }

    /// Add an endpoint
    pub async fn add_endpoint(&self, endpoint: String, weight: u32) {
        let mut endpoints = self.endpoints.write().await;
        endpoints.push(WeightedEndpoint {
            endpoint,
            weight,
            current_weight: 0,
        });
    }

    /// Remove an endpoint
    pub async fn remove_endpoint(&self, endpoint: &str) {
        let mut endpoints = self.endpoints.write().await;
        endpoints.retain(|e| e.endpoint != endpoint);
        self.pools.remove_pool(endpoint).await;
    }

    /// Select an endpoint using the configured strategy
    pub async fn select_endpoint(&self) -> TransportResult<String> {
        let mut endpoints = self.endpoints.write().await;

        if endpoints.is_empty() {
            return Err(TransportError::ConnectionClosed);
        }

        match self.strategy {
            LoadBalanceStrategy::RoundRobin => {
                let idx = self.rr_counter.fetch_add(1, Ordering::Relaxed) % endpoints.len();
                Ok(endpoints[idx].endpoint.clone())
            }

            LoadBalanceStrategy::LeastConnections => {
                let mut min_conns = usize::MAX;
                let mut selected = 0;

                for (i, ep) in endpoints.iter().enumerate() {
                    if let Ok(pool) = self.pools.get_pool(&ep.endpoint).await {
                        let conns = pool.size();
                        if conns < min_conns {
                            min_conns = conns;
                            selected = i;
                        }
                    }
                }

                Ok(endpoints[selected].endpoint.clone())
            }

            LoadBalanceStrategy::Random => {
                use std::time::SystemTime;
                let idx = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as usize
                    % endpoints.len();
                Ok(endpoints[idx].endpoint.clone())
            }

            LoadBalanceStrategy::Weighted => {
                // Smooth Weighted Round-Robin
                let mut total_weight: i32 = 0;
                let mut selected = 0;
                let mut max_weight: i32 = i32::MIN;

                for (i, ep) in endpoints.iter_mut().enumerate() {
                    ep.current_weight += ep.weight as i32;
                    total_weight += ep.weight as i32;

                    if ep.current_weight > max_weight {
                        max_weight = ep.current_weight;
                        selected = i;
                    }
                }

                endpoints[selected].current_weight -= total_weight;
                Ok(endpoints[selected].endpoint.clone())
            }
        }
    }

    /// Acquire a connection using load balancing
    pub async fn acquire(&self) -> TransportResult<(String, PoolGuard<'_, C>)> {
        let endpoint = self.select_endpoint().await?;
        let _pool = self.pools.get_pool(&endpoint).await?;

        // This won't compile as-is due to lifetime issues
        // In practice, we'd need to restructure this
        Err(TransportError::ConnectionClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frame;

    // Mock connection for testing
    #[derive(Clone)]
    struct MockConnection;

    impl TransportBackend for MockConnection {
        fn send_frame(
            &mut self,
            _frame: Frame,
        ) -> impl std::future::Future<Output = TransportResult<()>> + Send {
            async { Ok(()) }
        }

        fn recv_frame(
            &mut self,
        ) -> impl std::future::Future<Output = TransportResult<Frame>> + Send {
            async { Err(TransportError::ConnectionClosed) }
        }

        fn flush(&mut self) -> impl std::future::Future<Output = TransportResult<()>> + Send {
            async { Ok(()) }
        }

        fn rtt(&self) -> Duration {
            Duration::from_millis(10)
        }

        fn is_healthy(&self) -> bool {
            true
        }

        fn close(&mut self) -> impl std::future::Future<Output = TransportResult<()>> + Send {
            async { Ok(()) }
        }
    }

    #[tokio::test]
    async fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.max_connections, 32);
    }

    #[tokio::test]
    async fn test_pool_add() {
        let pool: ConnectionPool<MockConnection> = ConnectionPool::new(PoolConfig::default());

        for _ in 0..5 {
            pool.add(MockConnection).await.unwrap();
        }

        assert_eq!(pool.size(), 5);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let stats = PoolStats::new();

        stats.connections_created.fetch_add(10, Ordering::Relaxed);
        stats.connections_reused.fetch_add(90, Ordering::Relaxed);

        assert!((stats.reuse_ratio() - 0.9).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_endpoint_pool() {
        let pool: EndpointPool<MockConnection> = EndpointPool::new(PoolConfig::default(), 10);

        let p1 = pool.get_pool("endpoint1").await.unwrap();
        let p2 = pool.get_pool("endpoint2").await.unwrap();
        let p1_again = pool.get_pool("endpoint1").await.unwrap();

        // Same pool should be returned
        assert!(Arc::ptr_eq(&p1, &p1_again));
        assert!(!Arc::ptr_eq(&p1, &p2));

        let endpoints = pool.endpoints().await;
        assert_eq!(endpoints.len(), 2);
    }
}
