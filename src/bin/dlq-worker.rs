use devtools_dead_letter_worker::{
    infrai_client::{InfraiClient, InfraiError},
    poison_policy::{classify, BuildFailure, FailureDecision},
};
use std::env;
use thiserror::Error;

#[derive(Debug, Error)]
enum WorkerError {
    #[error(transparent)]
    Infrai(#[from] InfraiError),
    #[error("message {message_id} has an invalid build event: {source}")]
    InvalidBuildEvent {
        message_id: String,
        source: serde_json::Error,
    },
}

#[tokio::main]
async fn main() -> Result<(), WorkerError> {
    let task_url = env::var("BUILD_EVENT_URL")
        .unwrap_or_else(|_| "https://example.com/hooks/build-events".into());
    let infrai = InfraiClient::from_env()?;

    infrai.create_queue().await?;
    let cron = infrai.create_build_cron("*/10 * * * *", &task_url).await?;
    println!("build event cron ready: {}", cron.job_id);

    if env::args().any(|arg| arg == "--seed") {
        let failure = BuildFailure {
            build_id: "build-1842".into(),
            release_ref: "refs/tags/v2.4.1".into(),
            toolchain: "rust-stable".into(),
            attempt: 3,
            summary: "linker exited with status 1".into(),
        };
        infrai
            .publish(&failure, "build-event-build-1842-attempt-3")
            .await?;
        println!("build event published: {}", failure.build_id);
    }

    for message in infrai.consume(10, 60).await?.messages {
        let failure: BuildFailure = serde_json::from_value(message.payload).map_err(|source| {
            WorkerError::InvalidBuildEvent {
                message_id: message.message_id.clone(),
                source,
            }
        })?;

        match classify(failure.clone(), 3) {
            FailureDecision::Retry { next_attempt } => {
                println!(
                    "build {} remains retryable; next attempt {}",
                    failure.build_id, next_attempt
                );
            }
            FailureDecision::DeadLetter(diagnostic) => {
                let operation_id =
                    format!("diagnostic-{}-{}", diagnostic.build_id, failure.attempt);
                infrai.publish(&diagnostic, &operation_id).await?;
                infrai.ack(&message.message_id).await?;
                println!(
                    "release diagnostic published for {}",
                    diagnostic.release_ref
                );
            }
        }
    }
    Ok(())
}
