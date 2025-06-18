use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use uuid::Uuid;

// Message types for the chatroom
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    id: Uuid,
    username: String,
    message: String,
    timestamp: chrono::DateTime<chrono::Utc>,
    room: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum WebSocketMessage {
    #[serde(rename = "join")]
    Join { username: String, room: String },
    #[serde(rename = "message")]
    Message { message: String },
    #[serde(rename = "chat_message")]
    ChatMessage(ChatMessage),
    #[serde(rename = "user_joined")]
    UserJoined { username: String, room: String },
    #[serde(rename = "user_left")]
    UserLeft { username: String, room: String },
    #[serde(rename = "error")]
    Error { message: String },
}



// Application state
#[derive(Clone)]
struct AppState {
    db: PgPool,
    tx: broadcast::Sender<ChatMessage>,
    users: Arc<Mutex<HashMap<Uuid, UserInfo>>>,
}

#[derive(Debug, Clone)]
struct UserInfo {
    username: String,
    room: String,
}

#[derive(Deserialize)]
struct CreateMessage {
    username: String,
    message: String,
    room: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Load environment variables
    dotenv::dotenv().ok();

    // Database connection
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost:5432/ironvein".to_string());
    
    info!("Connecting to database...");
    let db = PgPool::connect(&database_url).await?;

    // Create messages table if it doesn't exist
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS messages (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            username VARCHAR NOT NULL,
            message TEXT NOT NULL,
            room VARCHAR NOT NULL,
            timestamp TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .execute(&db)
    .await?;

    info!("Database connected successfully!");

    // Create broadcast channel for real-time messages
    let (tx, _rx) = broadcast::channel(100);

    // Application state
    let state = AppState {
        db,
        tx,
        users: Arc::new(Mutex::new(HashMap::new())),
    };

    // Build our application with routes
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/ws/:room", get(websocket_handler))
        .route("/api/messages/:room", get(get_messages))
        .route("/api/messages", post(create_message))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Determine port
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    let addr = format!("0.0.0.0:{}", port);
    info!("🚀 IronVein Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// Route handlers
async fn index_handler() -> Html<&'static str> {
    Html(
        r#"
        <html>
            <head><title>IronVein Server</title></head>
            <body>
                <h1>🚀 IronVein Server</h1>
                <p>WebSocket Chatroom Server is running!</p>
                <ul>
                    <li><a href="/health">Health Check</a></li>
                    <li>WebSocket: <code>ws://localhost:8080/ws/general</code></li>
                    <li>API: <code>/api/messages/general</code></li>
                </ul>
            </body>
        </html>
        "#,
    )
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "ironvein-server",
        "timestamp": chrono::Utc::now()
    }))
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket_connection(socket, state))
}

async fn websocket_connection(socket: WebSocket, state: AppState) {
    let user_id = Uuid::new_v4();
    let mut rx = state.tx.subscribe();
    
    let (mut sender, mut receiver) = socket.split();
    
    // Spawn a task to handle incoming messages
    let tx = state.tx.clone();
    let db = state.db.clone();
    let users = state.users.clone();
    let user_id_clone = user_id;
    
    let receive_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            if let Ok(msg) = msg {
                if let Ok(text) = msg.to_text() {
                                         if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(text) {
                         match ws_msg {
                             WebSocketMessage::Join { username, room } => {
                                // Store user info
                                {
                                    let mut users_lock = users.lock().unwrap();
                                    users_lock.insert(user_id_clone, UserInfo {
                                        username: username.clone(),
                                        room: room.clone(),
                                    });
                                }
                                
                                info!("User {} joined room {}", username, room);
                                
                                // Broadcast user joined
                                let join_msg = ChatMessage {
                                    id: Uuid::new_v4(),
                                    username: "System".to_string(),
                                    message: format!("{} joined the chat", username),
                                    timestamp: chrono::Utc::now(),
                                    room: room.clone(),
                                };
                                
                                let _ = tx.send(join_msg);
                                                         }
                             WebSocketMessage::Message { message } => {
                                // Get user info
                                let user_info = {
                                    let users_lock = users.lock().unwrap();
                                    users_lock.get(&user_id_clone).cloned()
                                };
                                
                                if let Some(user_info) = user_info {
                                    let chat_message = ChatMessage {
                                        id: Uuid::new_v4(),
                                        username: user_info.username.clone(),
                                        message: message,
                                        timestamp: chrono::Utc::now(),
                                        room: user_info.room.clone(),
                                    };
                                    
                                    // Save to database
                                    let _ = sqlx::query(
                                        "INSERT INTO messages (id, username, message, room, timestamp) VALUES ($1, $2, $3, $4, $5)"
                                    )
                                    .bind(&chat_message.id)
                                    .bind(&chat_message.username)
                                    .bind(&chat_message.message)
                                    .bind(&chat_message.room)
                                    .bind(&chat_message.timestamp)
                                    .execute(&db)
                                    .await;
                                    
                                    // Broadcast message
                                    let _ = tx.send(chat_message);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    });
    
    // Spawn a task to handle outgoing messages
    let send_task = tokio::spawn(async move {
                 while let Ok(msg) = rx.recv().await {
             let ws_msg = WebSocketMessage::ChatMessage(msg);
            
            if let Ok(json) = serde_json::to_string(&ws_msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });
    
    // Wait for either task to finish
    tokio::select! {
        _ = receive_task => {},
        _ = send_task => {},
    }
    
    // Clean up user
    {
        let mut users_lock = state.users.lock().unwrap();
        if let Some(user_info) = users_lock.remove(&user_id) {
            info!("User {} left room {}", user_info.username, user_info.room);
            
            // Broadcast user left
            let leave_msg = ChatMessage {
                id: Uuid::new_v4(),
                username: "System".to_string(),
                message: format!("{} left the chat", user_info.username),
                timestamp: chrono::Utc::now(),
                room: user_info.room,
            };
            
            let _ = state.tx.send(leave_msg);
        }
    }
}

async fn get_messages(
    axum::extract::Path(room): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ChatMessage>>, StatusCode> {
    let messages = sqlx::query(
        "SELECT id, username, message, room, timestamp FROM messages WHERE room = $1 ORDER BY timestamp DESC LIMIT 50"
    )
    .bind(&room)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        warn!("Database error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let chat_messages: Vec<ChatMessage> = messages
        .into_iter()
        .map(|row| ChatMessage {
            id: row.get("id"),
            username: row.get("username"),
            message: row.get("message"),
            room: row.get("room"),
            timestamp: row.get("timestamp"),
        })
        .collect();

    Ok(Json(chat_messages))
}

async fn create_message(
    State(state): State<AppState>,
    Json(payload): Json<CreateMessage>,
) -> Result<Json<ChatMessage>, StatusCode> {
    let chat_message = ChatMessage {
        id: Uuid::new_v4(),
        username: payload.username,
        message: payload.message,
        timestamp: chrono::Utc::now(),
        room: payload.room,
    };

    sqlx::query(
        "INSERT INTO messages (id, username, message, room, timestamp) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(&chat_message.id)
    .bind(&chat_message.username)
    .bind(&chat_message.message)
    .bind(&chat_message.room)
    .bind(&chat_message.timestamp)
    .execute(&state.db)
    .await
    .map_err(|e| {
        warn!("Database error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Broadcast the message
    let _ = state.tx.send(chat_message.clone());

    Ok(Json(chat_message))
}
