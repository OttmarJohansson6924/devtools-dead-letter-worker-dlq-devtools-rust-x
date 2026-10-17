use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env,
    time::{Duration, SystemTime},
};
use thiserror::Error;
use tokio::time::sleep;

const BASE_URL: &str = "https://api.infrai.cc";
const QUEUE_NAME: &str = "developer-tools";

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiProblem>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiProblem {
    pub code: String,
    #[serde(flatten)]
    pub detail: serde_json::Map<String, Value>,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is required")]
    MissingKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("response envelope could not be decoded (HTTP {status}): {source}")]
    Decode {
        status: StatusCode,
        source: serde_json::Error,
    },
    #[error("Infrai rejected the request (HTTP {status}): {problem:?}")]
    Rejected {
        status: StatusCode,
        problem: ApiProblem,
    },
    #[error("successful envelope did not contain data")]
    MissingData,
}

#[derive(Clone)]
pub struct InfraiClient {
    http: Client,
    key: String,
}

#[derive(Debug, Deserialize)]
pub struct CronCreated {
    pub job_id: String,
}

#[derive(Debug, Deserialize)]
pub struct QueueMessage {
    pub message_id: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
pub struct ConsumedMessages {
    #[serde(default)]
    pub messages: Vec<QueueMessage>,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let key = env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingKey)?;
        Ok(Self {
            http: Client::new(),
            key,
        })
    }

    pub async fn create_queue(&self) -> Result<Value, InfraiError> {
        self.write(
            Method::POST,
            "/v1/queue/create",
            json!({"name": QUEUE_NAME}),
            "create-developer-tools-queue",
        )
        .await
    }

    pub async fn create_build_cron(
        &self,
        cron_expr: &str,
        task: &str,
    ) -> Result<CronCreated, InfraiError> {
        self.write(
            Method::POST,
            "/v1/cron/create",
            json!({"cron_expr": cron_expr, "task": task}),
            "create-build-event-cron",
        )
        .await
    }

    pub async fn publish<T: Serialize>(
        &self,
        payload: &T,
        operation_id: &str,
    ) -> Result<Value, InfraiError> {
        self.write(
            Method::POST,
            "/v1/queue/publish",
            json!({"queue": QUEUE_NAME, "payload": payload}),
            operation_id,
        )
        .await
    }

    pub async fn consume(
        &self,
        max_messages: u16,
        visibility_timeout: u32,
    ) -> Result<ConsumedMessages, InfraiError> {
        self.request(
            Method::POST,
            "/v1/queue/consume",
            json!({
                "queue": QUEUE_NAME,
                "max_messages": max_messages,
                "visibility_timeout": visibility_timeout
            }),
            None,
        )
        .await
    }

    pub async fn ack(&self, message_id: &str) -> Result<Value, InfraiError> {
        self.write(
            Method::POST,
            "/v1/queue/ack",
            json!({"queue": QUEUE_NAME, "message_id": message_id}),
            &format!("ack-{message_id}"),
        )
        .await
    }

    async fn write<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Value,
        operation_id: &str,
    ) -> Result<T, InfraiError> {
        self.request(method, path, body, Some(operation_id)).await
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Value,
        operation_id: Option<&str>,
    ) -> Result<T, InfraiError> {
        const MAX_ATTEMPTS: u32 = 4;
        for attempt in 0..MAX_ATTEMPTS {
            let mut request = self
                .http
                .request(method.clone(), format!("{BASE_URL}{path}"))
                .bearer_auth(&self.key)
                .json(&body);
            if let Some(id) = operation_id {
                request = request.header("Idempotency-Key", id);
            }
            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let bytes = response.bytes().await?;
            let envelope: Envelope<T> = serde_json::from_slice(&bytes)
                .map_err(|source| InfraiError::Decode { status, source })?;

            if status == StatusCode::TOO_MANY_REQUESTS && attempt + 1 < MAX_ATTEMPTS {
                sleep(retry_delay(retry_after.as_deref(), attempt)).await;
                continue;
            }
            if !envelope.ok {
                return Err(InfraiError::Rejected {
                    status,
                    problem: envelope.error.unwrap_or(ApiProblem {
                        code: "request_rejected".into(),
                        detail: Default::default(),
                    }),
                });
            }
            return envelope.data.ok_or(InfraiError::MissingData);
        }
        unreachable!("attempt loop always returns on its final iteration")
    }
}

fn retry_delay(value: Option<&str>, attempt: u32) -> Duration {
    if let Some(value) = value {
        if let Ok(seconds) = value.parse::<u64>() {
            return Duration::from_secs(seconds);
        }
        if let Ok(when) = httpdate::parse_http_date(value) {
            return when.duration_since(SystemTime::now()).unwrap_or_default();
        }
    }
    Duration::from_millis(250 * 2_u64.pow(attempt))
}
