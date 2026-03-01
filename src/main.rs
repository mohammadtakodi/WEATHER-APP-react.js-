mod config;
mod store;
mod worker;
mod fragmentation;
mod sanitizer;
mod context_manager;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};
use tracing::{info, error};
use uuid::Uuid;

use crate::{
    config::Config,
    context_manager::ContextManager,
    fragmentation::SemanticChunker,
    sanitizer::Sanitizer,
    store::RedisStore,
    worker::{Fragment, Worker},
};

struct AppState {
    store: Arc<RedisStore>,
    context_manager: Arc<ContextManager>,
    chunker: SemanticChunker,
    config: Arc<Config>,
}

#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    session_id: Option<String>, // New: Support for sessions
}

#[derive(Deserialize, Serialize, Clone)]
struct Message {
    role: String,
    content: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize Logging
    tracing_subscriber::fmt::init();
    info!("Starting ShadowMixer Rust Core...");

    // 2. Load Config
    let config = Arc::new(Config::from_env());
    info!("Config loaded. Redis: {}", config.redis_url);

    // 3. Initialize Components
    let store = Arc::new(RedisStore::new(&config.redis_url)?);
    let context_manager = Arc::new(ContextManager::new(store.clone(), 10)); // Keep last 10 messages
    let chunker = SemanticChunker::new(500, 50); // Chunk size 500 chars, overlap 50 chars
    
    // 4. Start Workers
    let worker = Arc::new(Worker::new(store.clone(), config.clone()));
    // Start 5 concurrent workers
    worker.start(5).await;

    // 5. Build Router
    let state = Arc::new(AppState {
        store,
        context_manager,
        chunker,
        config,
    });

    let app = Router::new()
        .route("/v1/secure/chat", post(handle_chat))
        .route("/v1/tasks/:id", get(handle_get_task))
        .with_state(state);

    // 6. Start Server
    let addr: SocketAddr = state.config.server_port.parse()?;
    info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn handle_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    
    // 1. Get latest user message
    let last_msg = match req.messages.last() {
        Some(msg) => msg,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "No messages provided"}))).into_response(),
    };

    // 2. Context Management
    // Retrieve previous context
    let _ = match state.context_manager.get_context(&session_id).await {
        Ok(msgs) => msgs,
        Err(e) => {
            error!("Failed to get context: {}", e);
            Vec::new()
        }
    };
    
    // Add new user message to context (async, don't block response too much)
    let _ = state.context_manager.add_message(&session_id, &last_msg.role, &last_msg.content).await;

    // 3. Sanitization (PII)
    let sanitized_content = if state.config.local_masking {
        Sanitizer::sanitize(&last_msg.content)
    } else {
        last_msg.content.clone()
    };

    // 4. Semantic Chunking
    let chunks = state.chunker.chunk(&sanitized_content);
    let total_chunks = chunks.len();
    let big_task_id = format!("task-{}", Uuid::new_v4());

    // 5. Queue Fragments
    for (i, chunk) in chunks.into_iter().enumerate() {
        // Construct prompt for this fragment
        // We can prepend context summary here if we had one.
        // For now, we just send the chunk.
        // Ideally, we might want to include some context in EACH chunk, 
        // but that increases token usage significantly.
        // Let's keep it simple: just the chunk. 
        // Or better: Prepend the LAST system message if exists.
        
        let fragment = Fragment {
            big_task_id: big_task_id.clone(),
            sequence_id: i,
            total: total_chunks,
            content: chunk,
            model: req.model.clone(),
        };

        let payload = serde_json::to_string(&fragment).unwrap();
        if let Err(e) = state.store.push_queue("llm_fragment_queue", &payload).await {
            error!("Failed to queue fragment: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Queue failed"}))).into_response();
        }
    }

    // Store metadata
    let _ = state.store.set_meta(&big_task_id, "total", &total_chunks.to_string()).await;
    let _ = state.store.set_meta(&big_task_id, "session_id", &session_id).await;

    Json(json!({
        "task_id": big_task_id,
        "session_id": session_id,
        "status": "queued",
        "fragments": total_chunks,
        "poll_url": format!("/v1/tasks/{}", big_task_id)
    })).into_response()
}

async fn handle_get_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    // Check total count
    let total_str = match state.store.get_meta(&task_id, "total").await {
        Ok(Some(v)) => v,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error": "Task not found"}))).into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Store error"}))).into_response(),
    };
    let total: usize = total_str.parse().unwrap_or(0);

    // Get results
    let results = match state.store.get_results(&task_id).await {
        Ok(res) => res,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };

    if results.len() < total {
        return Json(json!({
            "id": task_id,
            "status": "processing",
            "completed": results.len(),
            "total": total
        })).into_response();
    }

    // Reassemble
    let full_content = results.iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<&str>>()
        .join("\n\n"); // Simple join for now. Semantic reassembly is harder.

    // Update context with AI response
    // We need to know session_id.
    if let Ok(Some(session_id)) = state.store.get_meta(&task_id, "session_id").await {
        let _ = state.context_manager.add_message(&session_id, "assistant", &full_content).await;
    }

    Json(json!({
        "id": task_id,
        "status": "completed",
        "content": full_content
    })).into_response()
}
