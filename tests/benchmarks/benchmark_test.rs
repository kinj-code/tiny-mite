//! Phase 11 — Benchmark Integration Test
//!
//! Verifies the benchmark infrastructure works without requiring a real model.

#[cfg(test)]
mod tests {
    use super::super::tasks;
    use super::super::runner::BenchmarkConfig;
    use std::path::PathBuf;

    #[test]
    fn all_tasks_have_required_fields() {
        let tasks = tasks::all_tasks();
        assert!(!tasks.is_empty(), "Benchmark must have tasks");
        for task in &tasks {
            assert!(!task.id.is_empty(), "Task must have an ID");
            assert!(!task.prompt.is_empty(), "Task must have a prompt");
            assert!(task.timeout.as_secs() > 0, "Task must have a timeout");
        }
        println!("{} benchmark tasks validated", tasks.len());
    }

    #[test]
    fn benchmark_config_defaults() {
        let config = BenchmarkConfig::default();
        assert_eq!(config.model_name, "qwopus3.5-4b-coder-mtp");
        assert_eq!(config.provider_url, "http://localhost:1234");
        assert_eq!(config.trials_per_task, 3);
    }

    #[test]
    fn difficulty_distribution() {
        let tasks = tasks::all_tasks();
        let easy = tasks.iter().filter(|t| matches!(t.difficulty, tasks::Difficulty::Easy)).count();
        let medium = tasks.iter().filter(|t| matches!(t.difficulty, tasks::Difficulty::Medium)).count();
        let hard = tasks.iter().filter(|t| matches!(t.difficulty, tasks::Difficulty::Hard)).count();
        assert!(easy > 0, "Must have easy tasks");
        assert!(medium > 0, "Must have medium tasks");
        assert!(hard > 0, "Must have hard tasks");
        println!("Difficulty distribution: Easy={}, Medium={}, Hard={}", easy, medium, hard);
    }

    #[test]
    fn task_ids_are_unique() {
        let tasks = tasks::all_tasks();
        let mut ids: Vec<&str> = tasks.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), tasks.len(), "Task IDs must be unique");
    }

    #[test]
    fn easy_tasks_have_reasonable_timeouts() {
        let tasks = tasks::all_tasks();
        for task in &tasks {
            match task.difficulty {
                tasks::Difficulty::Easy => assert!(task.timeout.as_secs() <= 120, "Easy tasks should have short timeouts"),
                tasks::Difficulty::Medium => assert!(task.timeout.as_secs() <= 180, "Medium tasks timeout ok"),
                tasks::Difficulty::Hard => assert!(task.timeout.as_secs() <= 300, "Hard tasks timeout ok"),
            }
        }
    }
}