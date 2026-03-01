use crate::config::Config;
use crate::store::RedisStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use rand::Rng;
use std::time::Duration;
use tracing::{info, error, warn};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Fragment {
    pub big_task_id: String,
    pub sequence_id: usize,
    pub total: usize,
    pub content: String,
    pub model: String,
}

pub struct Worker {
    store: Arc<RedisStore>,
    config: Arc<Config>,
    key_idx: Mutex<usize>,
}

impl Worker {
    pub fn new(store: Arc<RedisStore>, config: Arc<Config>) -> Self {
        Self {
            store,
            config,
            key_idx: Mutex::new(0),
        }
    }

    pub async fn start(self: Arc<Self>, concurrency: usize) {
        info!("Starting {} worker threads...", concurrency);
        
        for id in 0..concurrency {
            let worker = self.clone();
            tokio::spawn(async move {
                worker.run_loop(id).await;
            });
        }
    }

    async fn run_loop(&self, worker_id: usize) {
        loop {
            // Blocking pop with timeout (e.g., 5 seconds) to allow graceful shutdown check if needed
            match self.store.pop_queue("llm_fragment_queue", 5).await {
                Ok(Some((_, payload))) => {
                    self.process_fragment(&payload).await;
                }
                Ok(None) => {
                    // Timeout, just continue loop
                }
                Err(e) => {
                    error!("[Worker {}] Redis error: {}", worker_id, e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn process_fragment(&self, payload: &str) {
        let fragment: Fragment = match serde_json::from_str(payload) {
            Ok(f) => f,
            Err(e) => {
                error!("Invalid JSON payload: {}", e);
                // Send to DLQ
                let _ = self.store.push_dlq(payload, &e.to_string()).await;
                return;
            }
        };

        info!("Processing fragment {}-{}", fragment.big_task_id, fragment.sequence_id);

        // 1. Jitter (Random Delay) - Non-blocking sleep
        let delay_ms = rand::thread_rng().gen_range(100..2000);
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;

        // 2. Key Selection (Round Robin)
        let api_key = {
            let mut idx = self.key_idx.lock().await;
            if self.config.llm_api_keys.is_empty() {
                error!("No API keys configured!");
                return;
            }
            let key = self.config.llm_api_keys[*idx].clone();
            *idx = (*idx + 1) % self.config.llm_api_keys.len();
            key
        };

        // 3. Call LLM
        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "model": fragment.model,
            "messages": [
                {"role": "user", "content": fragment.content}
            ]
        });

        // Simple retry logic
        let mut attempts = 0;
        let max_retries = 3;
        let mut response_content = String::new();
        let mut last_error = String::new();

        loop {
            attempts += 1;
            match client.post(&self.config.llm_target_url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await 
            {
                Ok(resp) => {
                    if resp.status().is_success() {
                        if let Ok(json_resp) = resp.json::<serde_json::Value>().await {
                            if let Some(choices) = json_resp.get("choices") {
                                if let Some(first) = choices.get(0) {
                                    if let Some(msg) = first.get("message") {
                                        if let Some(content) = msg.get("content") {
                                            response_content = content.as_str().unwrap_or("").to_string();
                                        }
                                    }
                                }
                            }
                            break; 
                        }
                    } else {
                        last_error = format!("HTTP {}", resp.status());
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                }
            }

            if attempts >= max_retries {
                error!("Failed to process fragment after {} attempts: {}", max_retries, last_error);
                // Send to DLQ
                let _ = self.store.push_dlq(payload, &last_error).await;
                // Save error result to prevent hanging
                let _ = self.store.save_result(&fragment.big_task_id, fragment.sequence_id, "[ERROR]").await;
                return;
            }
            
            // Exponential backoff
            tokio::time::sleep(Duration::from_secs(2u64.pow(attempts as u32))).await;
        }

        // 4. Save Result
        if let Err(e) = self.store.save_result(&fragment.big_task_id, fragment.sequence_id, &response_content).await {
            error!("Failed to save result: {}", e);
        } else {
            info!("Fragment {}-{} completed", fragment.big_task_id, fragment.sequence_id);
        }
    }
}
