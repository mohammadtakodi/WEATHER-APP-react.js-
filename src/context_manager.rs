use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use anyhow::Result;
use crate::store::RedisStore;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

pub struct ContextManager {
    store: Arc<RedisStore>,
    window_size: usize,
}

impl ContextManager {
    pub fn new(store: Arc<RedisStore>, window_size: usize) -> Self {
        Self { store, window_size }
    }

    pub async fn get_context(&self, session_id: &str) -> Result<Vec<Message>> {
        let key = format!("session:{}", session_id);
        let mut conn = self.store.client.get_async_connection().await?;
        
        // LRANGE returns list of strings. We want latest messages.
        // If we store with LPUSH, index 0 is newest.
        // So LRANGE 0 N-1 gives newest first. We probably want oldest first for LLM context.
        // Let's store newest at head (LPUSH). So to get context for LLM, we get 0..N, then reverse.
        let raw_msgs: Vec<String> = conn.lrange(key, 0, (self.window_size as isize) - 1).await?;
        
        let mut messages = Vec::new();
        for msg_str in raw_msgs {
            if let Ok(msg) = serde_json::from_str::<Message>(&msg_str) {
                messages.push(msg);
            }
        }
        // Reverse to chronological order (oldest first)
        messages.reverse();
        Ok(messages)
    }

    pub async fn add_message(&self, session_id: &str, role: &str, content: &str) -> Result<()> {
        let key = format!("session:{}", session_id);
        let msg = Message {
            role: role.to_string(),
            content: content.to_string(),
        };
        let msg_str = serde_json::to_string(&msg)?;
        
        let mut conn = self.store.client.get_async_connection().await?;
        conn.lpush(&key, msg_str).await?;
        conn.ltrim(&key, 0, (self.window_size as isize) - 1).await?;
        conn.expire(&key, 3600).await?;
        
        Ok(())
    }
}
