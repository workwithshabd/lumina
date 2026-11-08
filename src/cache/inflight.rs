use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use crate::cache::CachedImage;
use crate::error::AppError;

#[derive(Clone, Debug)]
pub enum InflightError {
    LeaderCancelled,
    Failed(Arc<AppError>),
}

pub type InflightResult = Result<Arc<CachedImage>, InflightError>;
pub type InflightSender = broadcast::Sender<InflightResult>;
pub type InflightReceiver = broadcast::Receiver<InflightResult>;

pub enum InflightRole<'a> {
    Leader(InflightLeaderGuard<'a>),
    Follower(InflightReceiver),
}

pub struct InflightLeaderGuard<'a> {
    manager: &'a InflightManager,
    key: String,
    tx: InflightSender,
    completed: bool,
}

impl<'a> InflightLeaderGuard<'a> {
    pub fn complete(mut self, result: InflightResult) {
        self.completed = true;
        {
            let mut map = self.manager.map.lock().unwrap_or_else(|e| e.into_inner());
            map.remove(&self.key);
        }
        let _ = self.tx.send(result);
    }
}

impl<'a> Drop for InflightLeaderGuard<'a> {
    fn drop(&mut self) {
        if !self.completed {
            {
                let mut map = self.manager.map.lock().unwrap_or_else(|e| e.into_inner());
                map.remove(&self.key);
            }
            // Explicitly notify waiting followers that the leader cancelled
            let _ = self.tx.send(Err(InflightError::LeaderCancelled));
        }
    }
}

/// Manages active in-flight requests to prevent thundering herd / cache stampede.
///
/// Guarantees that concurrent requests for the exact same cache key execute
/// only ONE origin fetch and image processing pipeline.
pub struct InflightManager {
    map: Mutex<HashMap<String, InflightSender>>,
}

impl InflightManager {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Checks if a request for `key` is already in flight.
    ///
    /// If yes, returns `InflightRole::Follower(rx)` to wait for completion.
    /// If no, registers this caller as `InflightRole::Leader(guard)`.
    pub fn subscribe_or_lead<'a>(&'a self, key: &str) -> InflightRole<'a> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = map.get(key) {
            InflightRole::Follower(tx.subscribe())
        } else {
            let (tx, _) = broadcast::channel(1);
            map.insert(key.to_string(), tx.clone());
            InflightRole::Leader(InflightLeaderGuard {
                manager: self,
                key: key.to_string(),
                tx,
                completed: false,
            })
        }
    }

    /// Returns the number of distinct keys currently in-flight.
    pub fn len(&self) -> usize {
        let map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        map.len()
    }

    /// Returns true if no in-flight requests are currently registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InflightManager {
    fn default() -> Self {
        Self::new()
    }
}
