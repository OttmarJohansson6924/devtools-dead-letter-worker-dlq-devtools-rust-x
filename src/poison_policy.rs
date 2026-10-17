use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BuildFailure {
    pub build_id: String,
    pub release_ref: String,
    pub toolchain: String,
    pub attempt: u8,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReleaseDiagnostic {
    pub kind: &'static str,
    pub build_id: String,
    pub release_ref: String,
    pub severity: &'static str,
    pub action: &'static str,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureDecision {
    Retry { next_attempt: u8 },
    DeadLetter(ReleaseDiagnostic),
}

pub fn classify(failure: BuildFailure, max_attempts: u8) -> FailureDecision {
    if failure.attempt < max_attempts {
        return FailureDecision::Retry {
            next_attempt: failure.attempt + 1,
        };
    }
    FailureDecision::DeadLetter(ReleaseDiagnostic {
        kind: "release_diagnostic",
        build_id: failure.build_id,
        release_ref: failure.release_ref,
        severity: "blocking",
        action: "inspect_build_log",
        summary: failure.summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_failure_becomes_a_release_diagnostic() {
        let failure = BuildFailure {
            build_id: "build-1842".into(),
            release_ref: "refs/tags/v2.4.1".into(),
            toolchain: "rust-stable".into(),
            attempt: 3,
            summary: "linker exited with status 1".into(),
        };

        let FailureDecision::DeadLetter(diagnostic) = classify(failure, 3) else {
            panic!("expected a dead-letter decision");
        };
        assert_eq!(diagnostic.release_ref, "refs/tags/v2.4.1");
        assert_eq!(diagnostic.severity, "blocking");
        assert_eq!(diagnostic.action, "inspect_build_log");
    }
}
