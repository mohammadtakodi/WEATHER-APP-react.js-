use redis::{AsyncCommands, Client, RedisError};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;

#[derive(Clone)]
pub struct RedisStore {
    pub client: Client,
    // We can use a connection pool in production, but for now simple client is okay
}

impl RedisStore {
    pub fn new(url: &str) -> Result<Self> {
        let client = Client::open(url)?;
        Ok(Self { client })
    }

    pub async fn push_queue(&self, queue_name: &str, message: &str) -> Result<()> {
        let mut conn = self.client.get_async_connection().await?;
        conn.rpush(queue_name, message).await?;
        Ok(())
    }

    pub async fn pop_queue(&self, queue_name: &str, timeout_secs: usize) -> Result<Option<String>> {
        let mut conn = self.client.get_async_connection().await?;
        // BLPOP returns (queue_name, value) or nil
        let result: Option<(String, String)> = conn.blpop(queue_name, timeout_secs).await?;
        Ok(result.map(|(_, v)| v))
    }

    pub async fn save_result(&self, task_id: &str, seq_id: usize, content: &str) -> Result<()> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("results:{}", task_id);
        conn.hset(key, seq_id.to_string(), content).await?;
        conn.expire(key, 86400).await?; // 24h TTL
        Ok(())
    }

    pub async fn get_results(&self, task_id: &str) -> Result<Vec<(usize, String)>> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("results:{}", task_id);
        let results: std::collections::HashMap<String, String> = conn.hgetall(key).await?;
        
        let mut parsed_results = Vec::new();
        for (k, v) in results {
            if let Ok(seq) = k.parse::<usize>() {
                parsed_results.push((seq, v));
            }
        }
        parsed_results.sort_by_key(|k| k.0);
        Ok(parsed_results)
    }

    pub async fn set_meta(&self, task_id: &str, key: &str, value: &str) -> Result<()> {
        let mut conn = self.client.get_async_connection().await?;
        let redis_key = format!("meta:{}:{}", task_id, key);
        conn.set(redis_key.clone(), value).await?;
        conn.expire(redis_key, 86400).await?;
        Ok(())
    }

    pub async fn get_meta(&self, task_id: &str, key: &str) -> Result<Option<String>> {
        let mut conn = self.client.get_async_connection().await?;
        let redis_key = format!("meta:{}:{}", task_id, key);
        let val: Option<String> = conn.get(redis_key).await?;
        Ok(val)
    }
    
    // Dead Letter Queue
    pub async fn push_dlq(&self, message: &str, error: &str) -> Result<()> {
        let mut conn = self.client.get_async_connection().await?;
        // We wrap the original message with error context
        let dlq_msg = serde_json::json!({
            "original": message,
            "error": error,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        conn.rpush("dlq:failed_fragments", dlq_msg.to_string()).await?;
        Ok(())
    }
}
