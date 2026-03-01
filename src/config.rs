use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server_port: String,
    pub redis_url: String,
    pub llm_api_keys: Vec<String>,
    pub llm_target_url: String,
    pub anonymization_level: String,
    pub local_masking: bool,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv::dotenv().ok();

        let server_port = env::var("SERVER_PORT").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/0".to_string());
        
        let api_keys_str = env::var("LLM_API_KEYS").unwrap_or_else(|_| "".to_string());
        let llm_api_keys: Vec<String> = api_keys_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let llm_target_url = env::var("LLM_TARGET_URL").unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
        
        let anonymization_level = env::var("ANONYMIZATION_LEVEL").unwrap_or_else(|_| "high".to_string());
        let local_masking = env::var("LOCAL_MASKING").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true);

        Config {
            server_port,
            redis_url,
            llm_api_keys,
            llm_target_url,
            anonymization_level,
            local_masking,
        }
    }
}
