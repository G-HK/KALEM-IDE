//! Test results for Kalem IDE.
//!
//! Every language's test runner can emit **JUnit XML** (Maven/Gradle for Java and Kotlin,
//! `pytest --junitxml`, `ctest --output-junit`, `cargo nextest`, `dotnet test` with a JUnit
//! logger, `gotestsum --junitfile`, Jest/Vitest JUnit reporters). Reading that one format gives the
//! Tests panel an overview for all languages. Language-native formats can be added later.

mod junit;

pub use junit::{parse_junit, JunitError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Passed,
    /// An assertion failed.
    Failed,
    /// The test could not run properly (exception, crash, setup failure).
    Error,
    Skipped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestCase {
    pub suite: String,
    pub class: Option<String>,
    pub name: String,
    pub duration_secs: f64,
    pub outcome: Outcome,
    /// Short failure message.
    pub message: Option<String>,
    /// Stack trace or longer failure text.
    pub details: Option<String>,
    /// Source file, if the runner reports it.
    pub file: Option<String>,
}

impl TestCase {
    /// `class.name` when a class is known, else just the name.
    pub fn full_name(&self) -> String {
        match &self.class {
            Some(c) if !c.is_empty() => format!("{c}.{}", self.name),
            _ => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub skipped: usize,
    pub duration_secs: f64,
}

impl Summary {
    /// True when nothing failed or errored (skipped tests do not count against a run).
    pub fn is_green(&self) -> bool {
        self.failed == 0 && self.errors == 0
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TestRun {
    pub cases: Vec<TestCase>,
}

impl TestRun {
    pub fn summary(&self) -> Summary {
        let mut s = Summary {
            total: self.cases.len(),
            ..Summary::default()
        };
        for c in &self.cases {
            s.duration_secs += c.duration_secs;
            match c.outcome {
                Outcome::Passed => s.passed += 1,
                Outcome::Failed => s.failed += 1,
                Outcome::Error => s.errors += 1,
                Outcome::Skipped => s.skipped += 1,
            }
        }
        s
    }

    /// Failed and errored tests, in report order.
    pub fn failures(&self) -> Vec<&TestCase> {
        self.cases
            .iter()
            .filter(|c| matches!(c.outcome, Outcome::Failed | Outcome::Error))
            .collect()
    }

    /// The `n` slowest tests, slowest first.
    pub fn slowest(&self, n: usize) -> Vec<&TestCase> {
        let mut v: Vec<&TestCase> = self.cases.iter().collect();
        v.sort_by(|a, b| b.duration_secs.total_cmp(&a.duration_secs));
        v.truncate(n);
        v
    }

    /// Per-suite summaries, in first-seen order. This is the tree level the panel groups by.
    pub fn by_suite(&self) -> Vec<(String, Summary)> {
        let mut order: Vec<String> = Vec::new();
        for c in &self.cases {
            if !order.contains(&c.suite) {
                order.push(c.suite.clone());
            }
        }
        order
            .into_iter()
            .map(|name| {
                let run = TestRun {
                    cases: self
                        .cases
                        .iter()
                        .filter(|c| c.suite == name)
                        .cloned()
                        .collect(),
                };
                (name, run.summary())
            })
            .collect()
    }

    /// Plain-text overview used by the CLI and as the model for the panel.
    pub fn overview(&self, slowest: usize) -> String {
        let s = self.summary();
        let mut out = String::new();
        out.push_str(&format!(
            "{}  {} tests: {} passed, {} failed, {} errors, {} skipped  ({:.2}s)\n",
            if s.is_green() { "PASS" } else { "FAIL" },
            s.total,
            s.passed,
            s.failed,
            s.errors,
            s.skipped,
            s.duration_secs
        ));
        for (suite, ss) in self.by_suite() {
            out.push_str(&format!(
                "  {suite}: {}/{} passed{}\n",
                ss.passed,
                ss.total,
                if ss.is_green() {
                    String::new()
                } else {
                    format!(", {} failing", ss.failed + ss.errors)
                }
            ));
        }
        let failures = self.failures();
        if !failures.is_empty() {
            out.push_str("\nFailures:\n");
            for f in failures {
                out.push_str(&format!("  - {}", f.full_name()));
                if let Some(m) = &f.message {
                    out.push_str(&format!(": {}", m.lines().next().unwrap_or("")));
                }
                out.push('\n');
            }
        }
        if slowest > 0 && !self.cases.is_empty() {
            out.push_str("\nSlowest:\n");
            for t in self.slowest(slowest) {
                out.push_str(&format!("  {:>8.3}s  {}\n", t.duration_secs, t.full_name()));
            }
        }
        out
    }
}
