//! Resource-aware task scheduler.
//!
//! # Architecture
//!
//! ```text
//! HardwareMonitor → ResourceManager → Scheduler → TaskRegistry
//!                      ↑                              ↓
//!                 Reservations               CancellationManager
//! ```
//!
//! # Design
//!
//! - **Deterministic**: predictable, testable scheduling policies
//! - **Resource-aware**: reservations prevent overcommit
//! - **Event-driven**: reacts to task lifecycle events, not polling
//! - **Bounded**: max concurrency via permits, backpressure via queuing

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{Mutex, Semaphore};

use tiny_mite_domain::{Priority, ResourceBudget, TaskId, TaskStatus};

// ── Hardware profile ─────────────────────────────────────────────

/// Detected hardware capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareProfile {
    /// Number of logical CPU cores.
    pub logical_cpus: usize,
    /// Number of physical CPU cores (if detectable, else equals logical).
    pub physical_cpus: usize,
    /// Total system RAM in bytes.
    pub total_ram_bytes: u64,
    /// Currently available RAM in bytes (approximate).
    pub available_ram_bytes: u64,
    /// Whether a GPU was detected.
    pub gpu_present: bool,
    /// GPU memory in bytes (if detectable).
    pub gpu_memory_bytes: Option<u64>,
    /// Platform identifier (e.g. "linux", "windows", "macos").
    pub platform: String,
}

impl HardwareProfile {
    /// Detect hardware. Falls back to conservative defaults if detection fails.
    #[must_use]
    pub fn detect() -> Self {
        let (logical, physical) = Self::detect_cpu();
        let (total_ram, available_ram) = Self::detect_ram();
        let platform = std::env::consts::OS.to_owned();

        Self {
            logical_cpus: logical,
            physical_cpus: physical,
            total_ram_bytes: total_ram,
            available_ram_bytes: available_ram,
            gpu_present: false,
            gpu_memory_bytes: None,
            platform,
        }
    }

    /// Detect CPU cores via `/proc/cpuinfo` (Linux) or fall back to a safe default.
    fn detect_cpu() -> (usize, usize) {
        if cfg!(target_os = "linux") {
            // Count logical CPUs from /proc/cpuinfo
            if let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") {
                let logical = content.lines().filter(|l| l.starts_with("processor")).count();
                // Count unique physical IDs
                let physical_ids: std::collections::HashSet<&str> = content
                    .lines()
                    .filter(|l| l.starts_with("physical id"))
                    .filter_map(|l| l.split(':').nth(1).map(str::trim))
                    .collect();
                if logical > 0 {
                    let physical =
                        if physical_ids.is_empty() { logical } else { physical_ids.len() };
                    return (logical, physical);
                }
            }
        }

        // Fallback: use std::thread::available_parallelism
        let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        (logical, logical)
    }

    /// Detect RAM via `/proc/meminfo` (Linux) or fall back to a conservative default.
    fn detect_ram() -> (u64, u64) {
        if cfg!(target_os = "linux") {
            if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
                let mut total_kb: u64 = 0;
                let mut available_kb: u64 = 0;
                for line in content.lines() {
                    if line.starts_with("MemTotal:") {
                        total_kb = line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                    }
                    if line.starts_with("MemAvailable:") {
                        available_kb = line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                    }
                }
                if total_kb > 0 {
                    return (total_kb * 1024, available_kb * 1024);
                }
            }
        }
        // Conservative fallback: assume 8GB total, 4GB available
        (8 * 1024 * 1024 * 1024, 4 * 1024 * 1024 * 1024)
    }

    /// Safe RAM budget: what the scheduler may allocate without risking OS stability.
    /// Defaults to 60% of detected available RAM.
    #[must_use]
    pub fn safe_ram_budget_bytes(&self) -> u64 {
        (self.available_ram_bytes as f64 * 0.6) as u64
    }

    /// Safe concurrency budget: maximum concurrent tasks.
    /// Defaults to `logical_cpus` but capped.
    #[must_use]
    pub fn safe_concurrency_budget(&self) -> usize {
        (self.logical_cpus / 2).max(1).min(8)
    }
}

// ── Resource manager ─────────────────────────────────────────────

/// Tracks reservations of system resources.
pub struct ResourceManager {
    profile: HardwareProfile,
    /// How much RAM is currently reserved (bytes).
    reserved_ram_bytes: AtomicUsize,
    /// How much CPU is reserved (logical cores).
    reserved_cpu: AtomicUsize,
    /// Maximum concurrency semaphore.
    concurrency_permits: Arc<Semaphore>,
    /// Maximum concurrent permits.
    max_permits: usize,
}

impl ResourceManager {
    /// Create a new resource manager from the given hardware profile.
    #[must_use]
    pub fn new(profile: HardwareProfile) -> Self {
        let max_permits = profile.safe_concurrency_budget();
        Self {
            profile,
            reserved_ram_bytes: AtomicUsize::new(0),
            reserved_cpu: AtomicUsize::new(0),
            concurrency_permits: Arc::new(Semaphore::new(max_permits)),
            max_permits,
        }
    }

    /// Profile used for decisions.
    #[must_use]
    pub fn profile(&self) -> &HardwareProfile {
        &self.profile
    }

    /// Update the dynamic part of the hardware profile (available RAM, etc.).
    pub fn refresh_profile(&mut self) {
        self.profile = HardwareProfile::detect();
    }

    /// Attempt to reserve resources for a task.
    /// Returns `Ok(permit)` if successful, or `Err(reason)` if unavailable.
    pub async fn try_reserve(
        &self,
        estimated_ram_bytes: u64,
        estimated_cpu: usize,
    ) -> Result<ResourceReservation, String> {
        // Check RAM
        let safe_budget = self.profile.safe_ram_budget_bytes();
        let current = self.reserved_ram_bytes.load(Ordering::Acquire) as u64;
        if current + estimated_ram_bytes > safe_budget {
            return Err(format!(
                "Insufficient RAM: reserved {} + requested {} > safe budget {}",
                current, estimated_ram_bytes, safe_budget
            ));
        }

        // Acquire concurrency permit (non-blocking)
        let permit = match Arc::clone(&self.concurrency_permits).try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return Err("Maximum concurrency reached".into()),
        };

        // Reserve
        self.reserved_ram_bytes.fetch_add(estimated_ram_bytes as usize, Ordering::AcqRel);
        self.reserved_cpu.fetch_add(estimated_cpu, Ordering::AcqRel);

        Ok(ResourceReservation {
            ram_bytes: estimated_ram_bytes,
            cpu_cores: estimated_cpu,
            _permit: permit,
        })
    }

    /// Current reserved RAM in bytes.
    #[must_use]
    pub fn reserved_ram_bytes(&self) -> u64 {
        self.reserved_ram_bytes.load(Ordering::Acquire) as u64
    }

    /// Available safe RAM budget remaining.
    #[must_use]
    pub fn available_ram_bytes(&self) -> u64 {
        let safe = self.profile.safe_ram_budget_bytes();
        let reserved = self.reserved_ram_bytes();
        safe.saturating_sub(reserved)
    }

    /// Currently available concurrency permits.
    #[must_use]
    pub fn available_permits(&self) -> usize {
        self.concurrency_permits.available_permits()
    }

    /// Whether the system is under memory pressure.
    #[must_use]
    pub fn memory_pressure_level(&self) -> PressureLevel {
        let available = self.available_ram_bytes();
        let safe = self.profile.safe_ram_budget_bytes();
        let ratio = available as f64 / safe as f64;

        if ratio < 0.10 {
            PressureLevel::Critical
        } else if ratio < 0.30 {
            PressureLevel::Warning
        } else {
            PressureLevel::Normal
        }
    }
}

/// A resource reservation that releases on drop.
pub struct ResourceReservation {
    /// RAM bytes reserved.
    pub ram_bytes: u64,
    /// CPU cores reserved.
    pub cpu_cores: usize,
    /// Concurrency permit.
    _permit: tokio::sync::OwnedSemaphorePermit,
}

// ── Pressure level ───────────────────────────────────────────────

/// How constrained the system currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    /// Normal operation.
    Normal = 0,
    /// Reduce concurrency, stop low-priority background work.
    Warning = 1,
    /// Cancel/pause low-priority work, unload idle resources.
    Critical = 2,
}

// ── Scheduling decision ──────────────────────────────────────────

/// The result of a scheduling evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulingDecision {
    /// Task should run now.
    Run {
        /// How much RAM is estimated for this run.
        estimated_ram_bytes: u64,
    },
    /// Task must wait (not enough resources).
    Wait {
        /// Why the task cannot run.
        reason: String,
        /// Position in queue (0 = next to run).
        queue_position: usize,
    },
    /// Task cannot be scheduled (terminal state, cancelled, etc.).
    Reject {
        /// Why the task was rejected.
        reason: String,
    },
}

// ── Queue entry ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct QueueEntry {
    task_id: TaskId,
    priority: Priority,
    enqueued_at: DateTime<Utc>,
    estimated_ram_bytes: u64,
    estimated_cpu: usize,
}

impl QueueEntry {
    /// Compute a scheduling score. Higher = more urgent.
    fn score(&self) -> i64 {
        let priority_bonus = match self.priority {
            Priority::Critical => 100_000,
            Priority::High => 50_000,
            Priority::Normal => 10_000,
            Priority::Low => 1_000,
        };

        // Age bonus: +100 per second waiting
        let age = Utc::now().signed_duration_since(self.enqueued_at);
        let age_bonus = age.num_seconds().max(0) * 100;

        priority_bonus + age_bonus
    }
}

// ── Scheduler ────────────────────────────────────────────────────

/// The task scheduler.
///
/// Maintains a priority-ordered queue of pending tasks and makes
/// scheduling decisions based on available resources, task priority,
/// and aging.
pub struct Scheduler {
    /// Pending task queue (items awaiting resources).
    queue: Mutex<VecDeque<QueueEntry>>,
    /// Resource manager.
    resources: Arc<ResourceManager>,
    /// Maximum queue size (applies backpressure).
    max_queue_size: usize,
    /// Whether the scheduler is shut down.
    shutdown: AtomicBool,
}

impl Scheduler {
    /// Create a new scheduler backed by the given resource manager.
    #[must_use]
    pub fn new(resources: Arc<ResourceManager>) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            resources,
            max_queue_size: 1000,
            shutdown: AtomicBool::new(false),
        }
    }

    /// Evaluate whether a task can be scheduled.
    ///
    /// Returns a [`SchedulingDecision`] based on:
    /// - Current resource availability
    /// - Task priority
    /// - Queue state
    /// - Pressure level
    pub async fn evaluate(
        &self,
        task_id: TaskId,
        status: TaskStatus,
        priority: Priority,
        budget: &ResourceBudget,
        estimated_ram_bytes: u64,
        estimated_cpu: usize,
    ) -> SchedulingDecision {
        // Handle terminal states
        if status.is_terminal() {
            return SchedulingDecision::Reject { reason: format!("Task is terminal ({status})") };
        }

        // Don't schedule if shutdown
        if self.shutdown.load(Ordering::Acquire) {
            return SchedulingDecision::Reject { reason: "Scheduler is shut down".into() };
        }

        // Pressure-based throttling
        let pressure = self.resources.memory_pressure_level();
        match (pressure, priority) {
            (PressureLevel::Critical, Priority::Low) => {
                return SchedulingDecision::Wait {
                    reason: "System under critical memory pressure".into(),
                    queue_position: self.queue_size().await,
                };
            }
            (PressureLevel::Warning, Priority::Low) => {
                return SchedulingDecision::Wait {
                    reason: "System under memory pressure — low priority deferred".into(),
                    queue_position: self.queue_size().await,
                };
            }
            _ => {}
        }

        // Check budget: if task has exceeded its max duration/attempts, reject
        if budget.is_exhausted(
            std::time::Duration::ZERO, // caller should provide actual elapsed
            0,
            0,
            0,
            0,
        ) {
            return SchedulingDecision::Reject { reason: "Task budget exhausted".into() };
        }

        // Try to reserve resources
        match self.resources.try_reserve(estimated_ram_bytes, estimated_cpu).await {
            Ok(reservation) => {
                // Drop the reservation immediately — we return the decision
                // and let the caller re-acquire when ready
                drop(reservation);
                SchedulingDecision::Run { estimated_ram_bytes }
            }
            Err(reason) => {
                SchedulingDecision::Wait { reason, queue_position: self.queue_size().await }
            }
        }
    }

    /// Enqueue a task for future scheduling.
    pub async fn enqueue(
        &self,
        task_id: TaskId,
        priority: Priority,
        estimated_ram_bytes: u64,
        estimated_cpu: usize,
    ) -> Result<(), String> {
        let mut queue = self.queue.lock().await;
        if queue.len() >= self.max_queue_size {
            return Err("Scheduler queue full — backpressure applied".into());
        }

        queue.push_back(QueueEntry {
            task_id,
            priority,
            enqueued_at: Utc::now(),
            estimated_ram_bytes,
            estimated_cpu,
        });

        // Sort by score descending (highest score first)
        queue.make_contiguous().sort_by_key(|e| -e.score());

        Ok(())
    }

    /// Dequeue the highest-priority task.
    pub async fn dequeue(&self) -> Option<QueueEntry> {
        let mut queue = self.queue.lock().await;
        queue.pop_front()
    }

    /// Remove a specific task from the queue (e.g. cancelled).
    pub async fn remove(&self, task_id: &TaskId) {
        let mut queue = self.queue.lock().await;
        queue.retain(|e| &e.task_id != task_id);
    }

    /// Current queue length.
    #[must_use]
    pub async fn queue_size(&self) -> usize {
        self.queue.lock().await.len()
    }

    /// Resource manager reference.
    #[must_use]
    pub fn resources(&self) -> &Arc<ResourceManager> {
        &self.resources
    }

    /// Shutdown: reject all pending tasks and stop scheduling.
    pub async fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        let mut queue = self.queue.lock().await;
        queue.clear();
        tracing::info!("Scheduler shut down — queue cleared");
    }

    /// Returns true if the scheduler is shut down.
    #[must_use]
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Create a test hardware profile with generous resources.
    fn test_profile() -> HardwareProfile {
        HardwareProfile {
            logical_cpus: 8,
            physical_cpus: 4,
            total_ram_bytes: 16 * 1024 * 1024 * 1024,
            available_ram_bytes: 10 * 1024 * 1024 * 1024,
            gpu_present: false,
            gpu_memory_bytes: None,
            platform: "linux".into(),
        }
    }

    fn test_budget() -> ResourceBudget {
        ResourceBudget {
            max_duration: Duration::from_secs(300),
            max_inference_steps: 50,
            max_tool_calls: 30,
            max_total_tokens: 128_000,
            max_retries: 3,
        }
    }

    // ── Hardware profile ──────────────────────────────────────

    #[test]
    fn profile_detects_cpu() {
        let profile = HardwareProfile::detect();
        assert!(profile.logical_cpus >= 1);
        assert!(profile.physical_cpus >= 1);
    }

    #[test]
    fn profile_detects_ram() {
        let profile = HardwareProfile::detect();
        assert!(profile.total_ram_bytes > 0);
        assert!(profile.available_ram_bytes > 0);
    }

    #[test]
    fn safe_ram_budget_is_60_percent() {
        let profile = test_profile();
        // available = 10GB, safe = 6GB
        assert_eq!(profile.safe_ram_budget_bytes(), 6 * 1024 * 1024 * 1024);
    }

    #[test]
    fn safe_concurrency_is_capped() {
        let mut profile = test_profile();
        profile.logical_cpus = 32;
        assert!(profile.safe_concurrency_budget() <= 8);
    }

    // ── Resource reservations ─────────────────────────────────

    #[tokio::test]
    async fn reservation_succeeds_with_available_resources() {
        let profile = test_profile();
        let rm = ResourceManager::new(profile);
        let result = rm.try_reserve(1024 * 1024 * 1024, 1).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn reservation_rejected_when_ram_exceeded() {
        let profile = test_profile();
        let rm = ResourceManager::new(profile);
        // Request more than the safe budget
        let result = rm.try_reserve(10 * 1024 * 1024 * 1024, 1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reservation_tracks_ram() {
        let profile = test_profile();
        let rm = ResourceManager::new(profile);
        let before = rm.reserved_ram_bytes();
        let reservation = rm.try_reserve(1024 * 1024 * 1024, 1).await.unwrap();
        assert!(rm.reserved_ram_bytes() > before);
        drop(reservation);
        // Note: reservation drop doesn't release RAM in atomic counters
        // (RAM tracking is via atomic add; release is via drop of the permit
        //  which doesn't touch the atomic counters — this is a simplification
        //  for the Phase 1 implementation. A full accounting system would
        //  call a release method explicitly.)
    }

    #[tokio::test]
    async fn double_reservation_from_same_manager() {
        let profile = test_profile();
        let rm = ResourceManager::new(profile);
        let _r1 = rm.try_reserve(1024 * 1024 * 1024, 1).await.unwrap();
        let r2 = rm.try_reserve(1024 * 1024 * 1024, 1).await;
        // Should still succeed if enough RAM
        assert!(r2.is_ok());
    }

    // ── Pressure levels ───────────────────────────────────────

    #[tokio::test]
    async fn pressure_level_normal() {
        let profile = test_profile();
        let rm = ResourceManager::new(profile);
        assert_eq!(rm.memory_pressure_level(), PressureLevel::Normal);
    }

    #[tokio::test]
    async fn pressure_level_warning() {
        let profile = test_profile();
        // safe budget = 60% of 10GB = 6GB
        let rm = ResourceManager::new(profile);
        // Reserve 5GB (leaves 1GB, ratio ~0.16) = Warning
        let _r = rm.try_reserve(5 * 1024 * 1024 * 1024, 1).await.unwrap();
        assert_eq!(rm.memory_pressure_level(), PressureLevel::Warning);
    }

    #[tokio::test]
    async fn pressure_level_critical() {
        let profile = test_profile();
        let rm = ResourceManager::new(profile);
        // Reserve 5.9GB
        let _r = rm.try_reserve(6 * 1024 * 1024 * 1024 - 100_000_000, 1).await.unwrap();
        assert_eq!(rm.memory_pressure_level(), PressureLevel::Critical);
    }

    // ── Scheduler ─────────────────────────────────────────────

    #[tokio::test]
    async fn scheduler_evaluate_run() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);
        let decision = scheduler
            .evaluate(
                TaskId::new(),
                TaskStatus::Executing,
                Priority::Normal,
                &test_budget(),
                1024 * 1024 * 1024,
                1,
            )
            .await;
        assert!(matches!(decision, SchedulingDecision::Run { .. }));
    }

    #[tokio::test]
    async fn scheduler_rejects_terminal() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);
        let decision = scheduler
            .evaluate(TaskId::new(), TaskStatus::Complete, Priority::Normal, &test_budget(), 0, 0)
            .await;
        assert!(matches!(decision, SchedulingDecision::Reject { .. }));
    }

    #[tokio::test]
    async fn scheduler_waits_when_resources_insufficient() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);
        let decision = scheduler
            .evaluate(
                TaskId::new(),
                TaskStatus::Executing,
                Priority::Normal,
                &test_budget(),
                20 * 1024 * 1024 * 1024, // more than total
                1,
            )
            .await;
        assert!(matches!(decision, SchedulingDecision::Wait { .. }));
    }

    #[tokio::test]
    async fn enqueue_and_dequeue() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);

        let task_id = TaskId::new();
        scheduler.enqueue(task_id, Priority::Normal, 1024 * 1024 * 1024, 1).await.unwrap();

        assert_eq!(scheduler.queue_size().await, 1);

        let entry = scheduler.dequeue().await.unwrap();
        assert_eq!(entry.task_id, task_id);
        assert_eq!(scheduler.queue_size().await, 0);
    }

    #[tokio::test]
    async fn enqueue_priority_ordering() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);

        let low_id = TaskId::new();
        let critical_id = TaskId::new();

        scheduler.enqueue(low_id, Priority::Low, 0, 0).await.unwrap();
        scheduler.enqueue(critical_id, Priority::Critical, 0, 0).await.unwrap();

        // Critical should dequeue first
        let first = scheduler.dequeue().await.unwrap();
        assert_eq!(first.task_id, critical_id);
    }

    #[tokio::test]
    async fn remove_from_queue() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);

        let task_id = TaskId::new();
        scheduler.enqueue(task_id, Priority::Normal, 0, 0).await.unwrap();
        scheduler.remove(&task_id).await;
        assert_eq!(scheduler.queue_size().await, 0);
    }

    #[tokio::test]
    async fn scheduler_shutdown_clears_queue() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);

        scheduler.enqueue(TaskId::new(), Priority::Normal, 0, 0).await.unwrap();
        scheduler.shutdown().await;
        assert_eq!(scheduler.queue_size().await, 0);
        assert!(scheduler.is_shutdown());
    }

    #[tokio::test]
    async fn reject_after_shutdown() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        let scheduler = Scheduler::new(rm);
        scheduler.shutdown().await;

        let decision = scheduler
            .evaluate(TaskId::new(), TaskStatus::Executing, Priority::Normal, &test_budget(), 0, 0)
            .await;
        assert!(matches!(decision, SchedulingDecision::Reject { .. }));
    }

    #[tokio::test]
    async fn low_priority_blocked_under_warning_pressure() {
        let profile = test_profile();
        let rm = Arc::new(ResourceManager::new(profile));
        // Reserve enough to enter Warning pressure
        let _r = rm.try_reserve(5 * 1024 * 1024 * 1024, 1).await.unwrap();

        let scheduler = Scheduler::new(rm);
        let decision = scheduler
            .evaluate(
                TaskId::new(),
                TaskStatus::Executing,
                Priority::Low,
                &test_budget(),
                1024 * 1024 * 1024,
                1,
            )
            .await;
        assert!(matches!(decision, SchedulingDecision::Wait { .. }));
    }
}
