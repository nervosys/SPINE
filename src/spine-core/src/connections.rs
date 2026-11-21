//! Shared admission for native TCP, WebSocket and optional QUIC connections.
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

pub struct ConnectionBudget {
    max_total: usize,
    max_per_ip: usize,
    counts: Mutex<HashMap<IpAddr, usize>>,
    active: AtomicUsize,
    rejected_global: AtomicU64,
    rejected_per_ip: AtomicU64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ConnectionSnapshot {
    pub active: usize,
    pub limit: usize,
    pub per_ip_limit: usize,
    pub rejected_global: u64,
    pub rejected_per_ip: u64,
}

impl ConnectionBudget {
    pub fn new(max_total: usize, max_per_ip: usize) -> Arc<Self> {
        Arc::new(Self {
            max_total,
            max_per_ip,
            counts: Mutex::new(HashMap::new()),
            active: AtomicUsize::new(0),
            rejected_global: AtomicU64::new(0),
            rejected_per_ip: AtomicU64::new(0),
        })
    }
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }
    pub fn snapshot(&self) -> ConnectionSnapshot {
        let _counts = self
            .counts
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ConnectionSnapshot {
            active: self.active(),
            limit: self.max_total,
            per_ip_limit: self.max_per_ip,
            rejected_global: self.rejected_global.load(Ordering::Relaxed),
            rejected_per_ip: self.rejected_per_ip.load(Ordering::Relaxed),
        }
    }
    pub fn try_acquire(self: &Arc<Self>, ip: IpAddr) -> Option<ConnectionLease> {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.active() >= self.max_total {
            self.rejected_global.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        if counts.get(&ip).copied().unwrap_or(0) >= self.max_per_ip {
            self.rejected_per_ip.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        *counts.entry(ip).or_default() += 1;
        self.active.fetch_add(1, Ordering::Relaxed);
        Some(ConnectionLease {
            budget: self.clone(),
            ip,
        })
    }
}

pub struct ConnectionLease {
    budget: Arc<ConnectionBudget>,
    ip: IpAddr,
}
impl Drop for ConnectionLease {
    fn drop(&mut self) {
        let mut counts = self
            .budget
            .counts
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(count) = counts.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.ip);
            }
            self.budget.active.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn admission_is_global_per_ip_and_released_on_task_abort() {
        let budget = ConnectionBudget::new(2, 1);
        let first = "127.0.0.1".parse().unwrap();
        let second = "127.0.0.2".parse().unwrap();
        let third = "127.0.0.3".parse().unwrap();
        let one = budget.try_acquire(first).unwrap();
        assert!(budget.try_acquire(first).is_none());
        let two = budget.try_acquire(second).unwrap();
        assert!(budget.try_acquire(third).is_none());
        let snapshot = budget.snapshot();
        assert_eq!(snapshot.active, 2);
        assert_eq!(snapshot.rejected_global, 1);
        assert_eq!(snapshot.rejected_per_ip, 1);
        let job = tokio::spawn(async move {
            let _lease = one;
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
        assert_eq!(budget.active(), 1);
        drop(two);
        assert_eq!(budget.active(), 0);
        assert!(budget.counts.lock().unwrap().is_empty());
        assert!(budget.try_acquire(first).is_some());
    }
}
