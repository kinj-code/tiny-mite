//! Runtime lifecycle coordinator.
//!
//! Manages the ordered initialization and graceful shutdown of Tiny Mite
//! subsystems. The lifecycle is:
//!
//! ```text
//! Created → Initializing → Ready → Running → Stopping → Stopped
//! ```
//!
//! Each state transition is observable and cancellable. Failures during
//! initialization produce structured errors with recovery guidance.

use std::sync::atomic::{AtomicU8, Ordering};
use tracing::{error, info};

use crate::error::DomainError;

// ---------------------------------------------------------------------------
// Lifecycle state
// ---------------------------------------------------------------------------

/// The states a runtime instance can occupy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LifecycleState {
    /// Initial state before any initialization.
    Created = 0,
    /// Subsystems are being initialized.
    Initializing = 1,
    /// All subsystems initialized; ready to accept work.
    Ready = 2,
    /// Actively processing tasks.
    Running = 3,
    /// Draining and shutting down.
    Stopping = 4,
    /// Fully stopped; no further work accepted.
    Stopped = 5,
}

impl LifecycleState {
    /// Returns `true` if the runtime can accept new work.
    #[must_use]
    pub fn can_accept_work(self) -> bool {
        matches!(self, Self::Ready | Self::Running)
    }

    /// Returns `true` if the runtime is at or past the stopping phase.
    #[must_use]
    pub fn is_shutting_down(self) -> bool {
        matches!(self, Self::Stopping | Self::Stopped)
    }
}

// ---------------------------------------------------------------------------
// Atomic state holder (lock-free for fast reads)
// ---------------------------------------------------------------------------

struct AtomicState(AtomicU8);

impl std::fmt::Debug for AtomicState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AtomicState").field(&self.load()).finish()
    }
}

impl AtomicState {
    const fn new() -> Self {
        Self(AtomicU8::new(LifecycleState::Created as u8))
    }

    fn load(&self) -> LifecycleState {
        // SAFETY: values are always from the LifecycleState repr
        match self.0.load(Ordering::Acquire) {
            0 => LifecycleState::Created,
            1 => LifecycleState::Initializing,
            2 => LifecycleState::Ready,
            3 => LifecycleState::Running,
            4 => LifecycleState::Stopping,
            5 => LifecycleState::Stopped,
            _ => LifecycleState::Stopped, // defensive
        }
    }

    fn store(&self, state: LifecycleState) {
        self.0.store(state as u8, Ordering::Release);
    }

    /// Attempt to transition from `expected` to `next`.
    /// Returns `true` if the transition succeeded.
    fn compare_exchange(
        &self,
        expected: LifecycleState,
        next: LifecycleState,
    ) -> Result<LifecycleState, LifecycleState> {
        match self.0.compare_exchange(
            expected as u8,
            next as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(next),
            Err(actual) => {
                // Decode actual state
                let actual_state = match actual {
                    0 => LifecycleState::Created,
                    1 => LifecycleState::Initializing,
                    2 => LifecycleState::Ready,
                    3 => LifecycleState::Running,
                    4 => LifecycleState::Stopping,
                    5 => LifecycleState::Stopped,
                    _ => LifecycleState::Stopped,
                };
                Err(actual_state)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime handle — shared by all components
// ---------------------------------------------------------------------------

/// A lightweight handle to the runtime lifecycle, safe to share across threads.
#[derive(Debug)]
pub struct RuntimeHandle {
    state: AtomicState,
}

impl RuntimeHandle {
    /// Create a new handle in the `Created` state.
    #[must_use]
    pub fn new() -> Self {
        Self { state: AtomicState::new() }
    }

    /// Query the current lifecycle state.
    #[must_use]
    pub fn state(&self) -> LifecycleState {
        self.state.load()
    }

    /// Transition from `Created` to `Initializing`.
    pub fn start_initializing(&self) -> Result<(), DomainError> {
        let _span = crate::component_span!("lifecycle");
        self.state
            .compare_exchange(LifecycleState::Created, LifecycleState::Initializing)
            .map(|_| {
                info!("Runtime entering Initializing state");
            })
            .map_err(|actual| {
                DomainError::permanent(format!("Cannot start initializing from state {actual:?}"))
            })
    }

    /// Transition to `Ready`.
    pub fn mark_ready(&self) -> Result<(), DomainError> {
        let _span = crate::component_span!("lifecycle");
        self.state
            .compare_exchange(LifecycleState::Initializing, LifecycleState::Ready)
            .map(|_| info!("Runtime Ready"))
            .map_err(|actual| {
                DomainError::permanent(format!("Cannot mark ready from state {actual:?}"))
            })
    }

    /// Transition to `Running`.
    pub fn mark_running(&self) -> Result<(), DomainError> {
        let _span = crate::component_span!("lifecycle");
        self.state
            .compare_exchange(LifecycleState::Ready, LifecycleState::Running)
            .or_else(|_| {
                self.state.compare_exchange(LifecycleState::Initializing, LifecycleState::Running)
            })
            .map(|_| info!("Runtime Running"))
            .map_err(|actual| {
                DomainError::permanent(format!("Cannot mark running from state {actual:?}"))
            })
    }

    /// Request graceful shutdown. This transitions through `Stopping` to `Stopped`.
    /// Idempotent — multiple calls are safe.
    pub fn shutdown(&self) {
        let _span = crate::component_span!("lifecycle");

        // Try to move to Stopping from Ready or Running
        let current = self.state.load();
        if !current.can_accept_work() && current != LifecycleState::Initializing {
            // Already stopping or stopped — nothing to do
            return;
        }

        self.state.store(LifecycleState::Stopping);
        info!("Runtime entering Stopping state — draining work");

        // In a real implementation, we would signal all subsystems here.
        // For Phase 1, we immediately transition to Stopped.

        self.state.store(LifecycleState::Stopped);
        info!("Runtime Stopped");
    }
}

impl Default for RuntimeHandle {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Component initializer trait
// ---------------------------------------------------------------------------

/// A subsystem that participates in the ordered startup process.
#[async_trait::async_trait]
pub trait Component: Send + Sync {
    /// Human-readable name (used in diagnostics).
    fn name(&self) -> &'static str;

    /// Perform initialization. Called once during startup.
    ///
    /// # Errors
    ///
    /// If initialization fails, the runtime startup should be aborted.
    async fn init(&self, handle: &RuntimeHandle) -> Result<(), DomainError>;

    /// Perform graceful shutdown. Called during `Stopping`.
    async fn shutdown(&self) -> Result<(), DomainError> {
        let _ = self;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Runtime coordinator
// ---------------------------------------------------------------------------

/// Orchestrates the startup and shutdown of all components.
pub struct Runtime {
    handle: RuntimeHandle,
    components: Vec<Box<dyn Component>>,
}

impl Runtime {
    /// Create a new runtime with the given components.
    /// Components are initialized in the order they appear.
    #[must_use]
    pub fn new(components: Vec<Box<dyn Component>>) -> Self {
        Self { handle: RuntimeHandle::new(), components }
    }

    /// Return a cloneable handle to the runtime state.
    #[must_use]
    pub fn handle(&self) -> &RuntimeHandle {
        &self.handle
    }

    /// Initialize all components in order. On failure, already-initialized
    /// components are shut down before returning the error.
    pub async fn startup(&self) -> Result<(), DomainError> {
        let _span = crate::component_span!("runtime-startup");

        self.handle.start_initializing()?;

        let mut initialized: Vec<usize> = Vec::with_capacity(self.components.len());

        for (i, component) in self.components.iter().enumerate() {
            let name = component.name();
            info!("Initializing component: {name}");
            match component.init(&self.handle).await {
                Ok(()) => {
                    info!("Component {name} initialized");
                    initialized.push(i);
                }
                Err(e) => {
                    error!(
                        component = name,
                        error = %e,
                        "Component initialization failed — rolling back"
                    );
                    // Shut down already-initialized components in reverse order
                    for &j in initialized.iter().rev() {
                        let _ = self.components[j].shutdown().await;
                    }
                    return Err(e.with_user_action(format!(
                        "Component '{name}' failed to start. Check configuration and retry."
                    )));
                }
            }
        }

        self.handle.mark_ready()?;
        self.handle.mark_running()?;
        Ok(())
    }

    /// Trigger graceful shutdown of all components in reverse order.
    pub async fn shutdown(&self) {
        self.handle.shutdown();
        for component in self.components.iter().rev() {
            let name = component.name();
            info!("Shutting down component: {name}");
            if let Err(e) = component.shutdown().await {
                error!(component = name, error = %e, "Error during component shutdown");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    struct NoopComponent {
        name: &'static str,
    }

    #[async_trait::async_trait]
    impl Component for NoopComponent {
        fn name(&self) -> &'static str {
            self.name
        }
        async fn init(&self, _handle: &RuntimeHandle) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[test]
    fn lifecycle_states_correctly_identify_work_acceptance() {
        assert!(!LifecycleState::Created.can_accept_work());
        assert!(!LifecycleState::Initializing.can_accept_work());
        assert!(LifecycleState::Ready.can_accept_work());
        assert!(LifecycleState::Running.can_accept_work());
        assert!(!LifecycleState::Stopping.can_accept_work());
        assert!(!LifecycleState::Stopped.can_accept_work());
    }

    #[test]
    fn shutdown_states_are_detected() {
        assert!(!LifecycleState::Ready.is_shutting_down());
        assert!(LifecycleState::Stopping.is_shutting_down());
        assert!(LifecycleState::Stopped.is_shutting_down());
    }

    #[tokio::test]
    async fn successful_startup_and_shutdown() {
        let runtime = Runtime::new(vec![
            Box::new(NoopComponent { name: "component-a" }),
            Box::new(NoopComponent { name: "component-b" }),
        ]);

        runtime.startup().await.expect("startup should succeed");
        assert_eq!(runtime.handle().state(), LifecycleState::Running);

        runtime.shutdown().await;
        assert_eq!(runtime.handle().state(), LifecycleState::Stopped);
    }

    struct FailingComponent;

    #[async_trait::async_trait]
    impl Component for FailingComponent {
        fn name(&self) -> &'static str {
            "failing-component"
        }
        async fn init(&self, _handle: &RuntimeHandle) -> Result<(), DomainError> {
            Err(DomainError::permanent("simulated failure"))
        }
    }

    #[tokio::test]
    async fn failing_component_rolls_back_previous() {
        let runtime =
            Runtime::new(vec![Box::new(NoopComponent { name: "a" }), Box::new(FailingComponent)]);

        let result = runtime.startup().await;
        assert!(result.is_err());
        // The runtime should NOT be marked Ready/Running
        assert_eq!(runtime.handle().state(), LifecycleState::Initializing);
    }

    #[test]
    fn double_shutdown_is_idempotent() {
        let handle = RuntimeHandle::new();
        handle.start_initializing().unwrap();
        handle.mark_ready().unwrap();
        handle.mark_running().unwrap();

        handle.shutdown();
        assert_eq!(handle.state(), LifecycleState::Stopped);

        // Second call should not panic
        handle.shutdown();
        assert_eq!(handle.state(), LifecycleState::Stopped);
    }

    #[test]
    fn cannot_start_initializing_from_running() {
        let handle = RuntimeHandle::new();
        handle.start_initializing().unwrap();
        handle.mark_ready().unwrap();
        handle.mark_running().unwrap();

        let result = handle.start_initializing();
        assert!(result.is_err());
    }
}
