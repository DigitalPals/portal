//! Snippet execution manager for tracking command execution on multiple hosts
//!
//! Manages the lifecycle of snippet executions, including tracking
//! in-flight operations and storing results per host.

use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

/// Status of command execution on a single host
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// Waiting to start
    Pending,
    /// Currently executing
    Running,
    /// Completed successfully
    Success,
    /// Failed with error message
    Failed(String),
}

/// Result of executing a snippet command on a single host
#[derive(Debug, Clone)]
pub struct HostResult {
    pub host_id: Uuid,
    pub host_name: String,
    pub status: ExecutionStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    /// UI state: is output expanded?
    pub expanded: bool,
}

/// Tracks a single snippet execution across multiple hosts
#[derive(Debug, Clone)]
pub struct SnippetExecution {
    pub snippet_id: Uuid,
    pub snippet_name: String,
    pub command: String,
    pub host_results: Vec<HostResult>,
    pub completed: bool,
}

impl SnippetExecution {
    /// Create a new execution tracker for the given hosts
    pub fn new(
        snippet_id: Uuid,
        snippet_name: String,
        command: String,
        hosts: Vec<(Uuid, String)>,
    ) -> Self {
        let host_results = hosts
            .into_iter()
            .map(|(id, name)| HostResult {
                host_id: id,
                host_name: name,
                status: ExecutionStatus::Pending,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
                duration: Duration::ZERO,
                expanded: false,
            })
            .collect();

        Self {
            snippet_id,
            snippet_name,
            command,
            host_results,
            completed: false,
        }
    }

    /// Check if all hosts have completed execution
    pub fn all_complete(&self) -> bool {
        self.host_results.iter().all(|r| {
            matches!(
                r.status,
                ExecutionStatus::Success | ExecutionStatus::Failed(_)
            )
        })
    }

    /// Count of successful executions
    pub fn success_count(&self) -> usize {
        self.host_results
            .iter()
            .filter(|r| r.status == ExecutionStatus::Success)
            .count()
    }

    /// Count of failed executions
    pub fn failure_count(&self) -> usize {
        self.host_results
            .iter()
            .filter(|r| matches!(r.status, ExecutionStatus::Failed(_)))
            .count()
    }

    /// Get mutable reference to a host result
    pub fn get_host_result_mut(&mut self, host_id: Uuid) -> Option<&mut HostResult> {
        self.host_results.iter_mut().find(|r| r.host_id == host_id)
    }
}

/// Manages snippet executions and their results
#[derive(Debug)]
pub struct SnippetExecutionManager {
    /// Currently active execution per snippet (only one per snippet at a time)
    active: HashMap<Uuid, SnippetExecution>,
    /// Most recent completed execution per snippet (for showing results)
    last_result: HashMap<Uuid, SnippetExecution>,
}

impl SnippetExecutionManager {
    /// Create a new empty execution manager
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
            last_result: HashMap::new(),
        }
    }

    /// Start a new execution for a snippet
    /// Returns the snippet_id for tracking
    pub fn start_execution(&mut self, execution: SnippetExecution) -> Uuid {
        let snippet_id = execution.snippet_id;
        self.active.insert(snippet_id, execution);
        snippet_id
    }

    /// Get reference to an active execution
    pub fn get_active(&self, snippet_id: Uuid) -> Option<&SnippetExecution> {
        self.active.get(&snippet_id)
    }

    /// Get mutable reference to an active execution
    pub fn get_active_mut(&mut self, snippet_id: Uuid) -> Option<&mut SnippetExecution> {
        self.active.get_mut(&snippet_id)
    }

    /// Check if a snippet is currently running
    pub fn is_running(&self, snippet_id: Uuid) -> bool {
        self.active.contains_key(&snippet_id)
    }

    /// Mark an execution as complete and move to last_result
    pub fn complete_execution(&mut self, snippet_id: Uuid) {
        if let Some(mut execution) = self.active.remove(&snippet_id) {
            execution.completed = true;
            self.last_result.insert(snippet_id, execution);
        }
    }

    /// Get the most recent execution result for a snippet
    pub fn get_last_result(&self, snippet_id: Uuid) -> Option<&SnippetExecution> {
        // Check active first, then last_result
        self.active
            .get(&snippet_id)
            .or_else(|| self.last_result.get(&snippet_id))
    }

    /// Get mutable reference to the most recent execution result
    pub fn get_last_result_mut(&mut self, snippet_id: Uuid) -> Option<&mut SnippetExecution> {
        // Check active first, then last_result
        if self.active.contains_key(&snippet_id) {
            self.active.get_mut(&snippet_id)
        } else {
            self.last_result.get_mut(&snippet_id)
        }
    }

    /// Clear results for a snippet
    pub fn clear_results(&mut self, snippet_id: Uuid) {
        self.last_result.remove(&snippet_id);
    }
}

impl Default for SnippetExecutionManager {
    fn default() -> Self {
        Self::new()
    }
}
