//! Phase 11 — Benchmark Task Definitions

use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty { Easy, Medium, Hard }

/// A benchmark task definition.
pub struct Task {
    pub id: &'static str,
    pub prompt: &'static str,
    pub difficulty: Difficulty,
    pub timeout: Duration,
    /// Whether the agent succeeded by verifying the output files/tests.
    pub expected_files: &'static [&'static str],
    /// How to validate: true if the task succeeded.
    pub validate: fn(&Path) -> bool,
}

pub fn all_tasks() -> Vec<Task> {
    vec![
        create_file(),
        modify_file(),
        read_and_edit(),
        python_hello(),
        rust_hello(),
        test_and_fix(),
        diagnose_bug(),
        multi_file(),
        git_status(),
        python_project(),
    ]
}

// ── Task 1: Create a file ─────────────────────────────────

fn create_file() -> Task {
    Task {
        id: "01_create_file",
        prompt: "Create /tmp/tiny-mite-bench-01.txt containing exactly 'Benchmark Phase 11 task 1'",
        difficulty: Difficulty::Easy,
        timeout: Duration::from_secs(90),
        expected_files: &["/tmp/tiny-mite-bench-01.txt"],
        validate: |dir| {
            std::fs::read_to_string(dir.join("tiny-mite-bench-01.txt"))
                .map(|s| s.trim() == "Benchmark Phase 11 task 1")
                .unwrap_or(false)
        },
    }
}

// ── Task 2: Modify an existing file ────────────────────────

fn modify_file() -> Task {
    Task {
        id: "02_modify_file",
        prompt: "Read /tmp/tiny-mite-bench-02.txt, then update it to add the line 'MODIFIED BY TINY MITE' at the end.",
        difficulty: Difficulty::Easy,
        timeout: Duration::from_secs(90),
        expected_files: &["/tmp/tiny-mite-bench-02.txt"],
        validate: |dir| {
            std::fs::read_to_string(dir.join("tiny-mite-bench-02.txt"))
                .map(|s| s.contains("MODIFIED BY TINY MITE"))
                .unwrap_or(false)
        },
    }
}

// ── Task 3: Read → understand → edit ───────────────────────

fn read_and_edit() -> Task {
    Task {
        id: "03_read_edit",
        prompt: "Read all files in /tmp. Find the latest modified text file and append a timestamp line to it.",
        difficulty: Difficulty::Medium,
        timeout: Duration::from_secs(120),
        expected_files: &[],
        validate: |_| true, // Hard to validate deterministically
    }
}

// ── Task 4: Create a small Python program ──────────────────

fn python_hello() -> Task {
    Task {
        id: "04_python_hello",
        prompt: "Create /tmp/tiny-mite-bench-04.py that prints 'Hello from Tiny Mite Benchmark' when run with python3.",
        difficulty: Difficulty::Medium,
        timeout: Duration::from_secs(120),
        expected_files: &["/tmp/tiny-mite-bench-04.py"],
        validate: |dir| {
            let path = dir.join("tiny-mite-bench-04.py");
            if !path.exists() { return false; }
            std::process::Command::new("python3")
                .arg(&path)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).contains("Hello from Tiny Mite Benchmark"))
                .unwrap_or(false)
        },
    }
}

// ── Task 5: Create a small Rust program ────────────────────

fn rust_hello() -> Task {
    Task {
        id: "05_rust_hello",
        prompt: "Create a Rust project at /tmp/tiny-mite-bench-05 with a main.rs that prints 'Tiny Mite Rust Benchmark' and compiles with cargo build.",
        difficulty: Difficulty::Hard,
        timeout: Duration::from_secs(180),
        expected_files: &["/tmp/tiny-mite-bench-05/Cargo.toml", "/tmp/tiny-mite-bench-05/src/main.rs"],
        validate: |dir| {
            let project = dir.join("tiny-mite-bench-05");
            if !project.join("Cargo.toml").exists() { return false; }
            std::process::Command::new("cargo")
                .args(["build", "--quiet"])
                .current_dir(&project)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        },
    }
}

// ── Task 6: Run tests and fix a failure ────────────────────

fn test_and_fix() -> Task {
    Task {
        id: "06_test_fix",
        prompt: "Create a Python calculator at /tmp/tiny-mite-bench-06 with functions add, subtract, multiply, divide. Create tests. Run the tests. If any fail, fix them and rerun.",
        difficulty: Difficulty::Hard,
        timeout: Duration::from_secs(300),
        expected_files: &["/tmp/tiny-mite-bench-06/calculator.py", "/tmp/tiny-mite-bench-06/test_calculator.py"],
        validate: |dir| {
            let test_dir = dir.join("tiny-mite-bench-06");
            if !test_dir.join("test_calculator.py").exists() { return false; }
            std::process::Command::new("python3")
                .args(["-m", "pytest", "-q"])
                .current_dir(&test_dir)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        },
    }
}

// ── Task 7: Diagnose a deliberately broken program ─────────

fn diagnose_bug() -> Task {
    Task {
        id: "07_diagnose_bug",
        prompt: "There is a bug in /tmp/tiny-mite-bench-07/broken.py. Find and fix it so the program runs correctly. The program should compute the sum of two numbers.",
        difficulty: Difficulty::Medium,
        timeout: Duration::from_secs(180),
        expected_files: &["/tmp/tiny-mite-bench-07/broken.py"],
        validate: |dir| {
            let path = dir.join("tiny-mite-bench-07/broken.py");
            if !path.exists() { return false; }
            std::process::Command::new("python3")
                .arg(&path)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        },
    }
}

// ── Task 8: Multi-file modification ────────────────────────

fn multi_file() -> Task {
    Task {
        id: "08_multifile",
        prompt: "Create 3 files in /tmp/tiny-mite-bench-08: config.json with {} content, main.py that reads config.json, and README.md explaining the project.",
        difficulty: Difficulty::Medium,
        timeout: Duration::from_secs(180),
        expected_files: &[
            "/tmp/tiny-mite-bench-08/config.json",
            "/tmp/tiny-mite-bench-08/main.py",
            "/tmp/tiny-mite-bench-08/README.md",
        ],
        validate: |dir| {
            let base = dir.join("tiny-mite-bench-08");
            base.join("config.json").exists()
                && base.join("main.py").exists()
                && base.join("README.md").exists()
        },
    }
}

// ── Task 9: Git-based modification task ────────────────────

fn git_status() -> Task {
    Task {
        id: "09_git_status",
        prompt: "Run git status in the current directory and report what you find. Create a file called /tmp/tiny-mite-bench-09-status.txt with the output.",
        difficulty: Difficulty::Easy,
        timeout: Duration::from_secs(90),
        expected_files: &["/tmp/tiny-mite-bench-09-status.txt"],
        validate: |dir| {
            dir.join("tiny-mite-bench-09-status.txt").exists()
        },
    }
}

// ── Task 10: Small project creation from specification ─────

fn python_project() -> Task {
    Task {
        id: "10_python_project",
        prompt: "Create a Python TODO CLI at /tmp/tiny-mite-bench-10 with todo.py implementing add, list, complete, and remove operations. Include tests.",
        difficulty: Difficulty::Hard,
        timeout: Duration::from_secs(300),
        expected_files: &["/tmp/tiny-mite-bench-10/todo.py", "/tmp/tiny-mite-bench-10/test_todo.py"],
        validate: |dir| {
            let base = dir.join("tiny-mite-bench-10");
            base.join("todo.py").exists() && base.join("test_todo.py").exists()
        },
    }
}