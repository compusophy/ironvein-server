use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Path,
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

// Game constants
const GRID_SIZE: u32 = 64;

// Game structures
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Player {
    username: String,
    x: u32,
    y: u32,
    room: String,
    health: u32,
    resources: u32,
}

// Message types for the MMO RTS
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
    // Chat system (legacy)
    #[serde(rename = "join")]
    Join { username: String, room: String },
    #[serde(rename = "message")]
    Message { username: String, message: String, room: String },
    #[serde(rename = "chat_message")]
    ChatMessage(ChatMessage),
    #[serde(rename = "user_joined")]
    UserJoined { username: String, room: String },
    #[serde(rename = "user_left")]
    UserLeft { username: String, room: String },
    #[serde(rename = "error")]
    Error { message: String },
    
    // Game system (new)
    #[serde(rename = "move")]
    Move { username: String, x: u32, y: u32, room: String },
    #[serde(rename = "player_update")]
    PlayerUpdate { username: String, x: u32, y: u32, health: u32, resources: u32 },
    #[serde(rename = "game_state")]
    GameState { players: Vec<Player> },
    #[serde(rename = "player_joined")]
    PlayerJoined { username: String, x: u32, y: u32 },
    #[serde(rename = "player_left")]
    PlayerLeft { username: String },
}

// Application state
#[derive(Clone)]
struct AppState {
    db: PgPool,
    chat_tx: broadcast::Sender<ChatMessage>,
    game_tx: broadcast::Sender<WebSocketMessage>,
    users: Arc<Mutex<HashMap<Uuid, UserInfo>>>,
    players: Arc<Mutex<HashMap<String, Player>>>, // username -> player
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

    // Create tables if they don't exist
    setup_database(&db).await?;

    info!("Database connected successfully!");

    // Create broadcast channels
    let (chat_tx, _chat_rx) = broadcast::channel(100);
    let (game_tx, _game_rx) = broadcast::channel(100);

    // Application state
    let state = AppState {
        db,
        chat_tx,
        game_tx,
        users: Arc::new(Mutex::new(HashMap::new())),
        players: Arc::new(Mutex::new(HashMap::new())),
    };

    // Build our application with routes
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/ws/:room", get(websocket_handler))
        .route("/api/messages/:room", get(get_messages))
        .route("/api/messages", post(create_message))
        .route("/api/players/:room", get(get_players))
        .route("/api/gamestate/:room", get(get_game_state))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Determine port
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    let addr = format!("0.0.0.0:{}", port);
    info!("🚀 IronVein MMO RTS Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn setup_database(db: &PgPool) -> anyhow::Result<()> {
    // Messages table (existing)
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
    .execute(db)
    .await?;

    // Players table (new for MMO RTS)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS players (
            id SERIAL PRIMARY KEY,
            username VARCHAR(50) UNIQUE NOT NULL,
            x INTEGER NOT NULL DEFAULT 0,
            y INTEGER NOT NULL DEFAULT 0,
            room VARCHAR(50) NOT NULL,
            health INTEGER NOT NULL DEFAULT 100,
            resources INTEGER NOT NULL DEFAULT 0,
            last_seen TIMESTAMPTZ DEFAULT NOW(),
            created_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .execute(db)
    .await?;

    // Game rooms table (new)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS game_rooms (
            room VARCHAR(50) PRIMARY KEY,
            grid_data JSONB,
            max_players INTEGER DEFAULT 50,
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .execute(db)
    .await?;

    info!("Database tables created successfully!");
    Ok(())
}

// Route handlers
async fn index_handler() -> Html<&'static str> {
    Html(
        r#"
        <html>
            <head><title>IronVein MMO RTS Server</title></head>
            <body style="font-family: Arial, sans-serif; margin: 40px; background: #1e3c72; color: white;">
                <h1>⚔️ IronVein MMO RTS Server</h1>
                <p>Real-time multiplayer strategy game server is running!</p>
                
                <h2>🎮 Game Features</h2>
                <ul>
                    <li>64x64 Grid Battlefield</li>
                    <li>Real-time Player Movement</li>
                    <li>Multi-room Support</li>
                    <li>Integrated Chat System</li>
                </ul>
                
                <h2>🔗 Endpoints</h2>
                <ul>
                    <li><a href="/health" style="color: #4ecdc4;">Health Check</a></li>
                    <li>WebSocket: <code>ws://localhost:8080/ws/general</code></li>
                    <li>Chat API: <code>/api/messages/general</code></li>
                    <li>Players API: <code>/api/players/general</code></li>
                    <li>Game State: <code>/api/gamestate/general</code></li>
                </ul>
                
                <h2>📊 Game Stats</h2>
                <p>Grid Size: 64x64 units</p>
                <p>Max Players per Room: 50</p>
            </body>
        </html>
        "#,
    )
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "ironvein-mmo-rts-server",
        "game_features": {
            "grid_size": GRID_SIZE,
            "max_players_per_room": 50,
            "real_time_movement": true,
            "chat_system": true
        },
        "timestamp": chrono::Utc::now()
    }))
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    Path(room): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket_connection(socket, state, room))
}

async fn websocket_connection(socket: WebSocket, state: AppState, room: String) {
    let user_id = Uuid::new_v4();
    let mut chat_rx = state.chat_tx.subscribe();
    let mut game_rx = state.game_tx.subscribe();
    
    let (mut sender, mut receiver) = socket.split();
    
    // Spawn a task to handle incoming messages
    let chat_tx = state.chat_tx.clone();
    let game_tx = state.game_tx.clone();
    let db = state.db.clone();
    let users = state.users.clone();
    let players = state.players.clone();
    let user_id_clone = user_id;
    let room_clone = room.clone();
    
    let receive_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            if let Ok(msg) = msg {
                if let Ok(text) = msg.to_text() {
                    if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(text) {
                        match ws_msg {
                            WebSocketMessage::Join { username, room } => {
                                info!("Player {} joining room {}", username, room);
                                
                                // Store user info
                                {
                                    let mut users_lock = users.lock().unwrap();
                                    users_lock.insert(user_id_clone, UserInfo {
                                        username: username.clone(),
                                        room: room.clone(),
                                    });
                                }
                                
                                // Generate random starting position
                                let x = (rand::random::<u32>() % GRID_SIZE).min(GRID_SIZE - 1);
                                let y = (rand::random::<u32>() % GRID_SIZE).min(GRID_SIZE - 1);
                                
                                let player = Player {
                                    username: username.clone(),
                                    x,
                                    y,
                                    room: room.clone(),
                                    health: 100,
                                    resources: 0,
                                };
                                
                                // Store player in memory
                                {
                                    let mut players_lock = players.lock().unwrap();
                                    players_lock.insert(username.clone(), player.clone());
                                }
                                
                                // Save/update player in database
                                let _ = sqlx::query(
                                    r#"
                                    INSERT INTO players (username, x, y, room, health, resources, last_seen)
                                    VALUES ($1, $2, $3, $4, $5, $6, NOW())
                                    ON CONFLICT (username) 
                                    DO UPDATE SET x = $2, y = $3, room = $4, health = $5, resources = $6, last_seen = NOW()
                                    "#
                                )
                                .bind(&username)
                                .bind(x as i32)
                                .bind(y as i32)
                                .bind(&room)
                                .bind(100i32)
                                .bind(0i32)
                                .execute(&db)
                                .await;
                                
                                // Broadcast player joined
                                let join_msg = WebSocketMessage::PlayerJoined { 
                                    username: username.clone(), 
                                    x, 
                                    y 
                                };
                                let _ = game_tx.send(join_msg);
                                
                                // Send current game state to new player
                                let all_players: Vec<Player> = {
                                    let players_lock = players.lock().unwrap();
                                    players_lock.values()
                                        .filter(|p| p.room == room)
                                        .cloned()
                                        .collect()
                                };
                                
                                let game_state_msg = WebSocketMessage::GameState { 
                                    players: all_players 
                                };
                                let _ = game_tx.send(game_state_msg);
                                
                                info!("Player {} spawned at ({}, {}) in room {}", username, x, y, room);
                            }
                            
                            WebSocketMessage::Move { username, x, y, room } => {
                                info!("Player {} moving to ({}, {}) in room {}", username, x, y, room);
                                
                                // Validate move (bounds check)
                                if x >= GRID_SIZE || y >= GRID_SIZE {
                                    let error_msg = WebSocketMessage::Error { 
                                        message: format!("Invalid position: ({}, {}). Grid size is {}x{}", x, y, GRID_SIZE, GRID_SIZE)
                                    };
                                    let _ = game_tx.send(error_msg);
                                    continue;
                                }
                                
                                // Update player position in memory
                                let mut updated_player = None;
                                {
                                    let mut players_lock = players.lock().unwrap();
                                    if let Some(player) = players_lock.get_mut(&username) {
                                        player.x = x;
                                        player.y = y;
                                        updated_player = Some(player.clone());
                                    }
                                }
                                
                                // Update database
                                let _ = sqlx::query(
                                    "UPDATE players SET x = $1, y = $2, last_seen = NOW() WHERE username = $3 AND room = $4"
                                )
                                .bind(x as i32)
                                .bind(y as i32)
                                .bind(&username)
                                .bind(&room)
                                .execute(&db)
                                .await;
                                
                                // Broadcast position update
                                if let Some(player) = updated_player {
                                    let update_msg = WebSocketMessage::PlayerUpdate {
                                        username: player.username,
                                        x: player.x,
                                        y: player.y,
                                        health: player.health,
                                        resources: player.resources,
                                    };
                                    let _ = game_tx.send(update_msg);
                                }
                            }
                            
                            WebSocketMessage::Message { username, message, room } => {
                                info!("Chat message from {} in {}: {}", username, room, message);
                                
                                let chat_message = ChatMessage {
                                    id: Uuid::new_v4(),
                                    username: username.clone(),
                                    message,
                                    timestamp: chrono::Utc::now(),
                                    room: room.clone(),
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
                                let _ = chat_tx.send(chat_message);
                            }
                            
                            _ => {
                                warn!("Unhandled message type: {:?}", ws_msg);
                            }
                        }
                    }
                }
            }
        }
    });
    
    // Spawn a task to handle outgoing messages
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Chat messages
                chat_result = chat_rx.recv() => {
                    if let Ok(msg) = chat_result {
                        // Only send chat messages for the same room
                        if msg.room == room {
                            let ws_msg = WebSocketMessage::ChatMessage(msg);
                            if let Ok(json) = serde_json::to_string(&ws_msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                
                // Game messages
                game_result = game_rx.recv() => {
                    if let Ok(msg) = game_result {
                        // Filter messages by room when applicable
                        let should_send = match &msg {
                            WebSocketMessage::PlayerUpdate { .. } => true,
                            WebSocketMessage::PlayerJoined { .. } => true,
                            WebSocketMessage::PlayerLeft { .. } => true,
                            WebSocketMessage::GameState { players } => {
                                // Only send if there are players in this room
                                players.iter().any(|p| p.room == room)
                            }
                            _ => true,
                        };
                        
                        if should_send {
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    });
    
    // Wait for either task to finish
    tokio::select! {
        _ = receive_task => {},
        _ = send_task => {},
    }
    
    // Clean up user and player
    let username_to_remove = {
        let mut users_lock = state.users.lock().unwrap();
        users_lock.remove(&user_id).map(|user_info| {
            info!("User {} left room {}", user_info.username, user_info.room);
            user_info.username
        })
    };
    
    if let Some(username) = username_to_remove {
        // Remove from players
        {
            let mut players_lock = state.players.lock().unwrap();
            players_lock.remove(&username);
        }
        
        // Update database
        let _ = sqlx::query("UPDATE players SET last_seen = NOW() WHERE username = $1")
            .bind(&username)
            .execute(&state.db)
            .await;
        
        // Broadcast player left
        let leave_msg = WebSocketMessage::PlayerLeft { username };
        let _ = state.game_tx.send(leave_msg);
    }
}

// API endpoints
async fn get_messages(
    Path(room): Path<String>,
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

async fn get_players(
    Path(room): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Player>>, StatusCode> {
    let players = sqlx::query(
        "SELECT username, x, y, room, health, resources FROM players WHERE room = $1 AND last_seen > NOW() - INTERVAL '5 minutes'"
    )
    .bind(&room)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        warn!("Database error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let game_players: Vec<Player> = players
        .into_iter()
        .map(|row| Player {
            username: row.get("username"),
            x: row.get::<i32, _>("x") as u32,
            y: row.get::<i32, _>("y") as u32,
            room: row.get("room"),
            health: row.get::<i32, _>("health") as u32,
            resources: row.get::<i32, _>("resources") as u32,
        })
        .collect();

    Ok(Json(game_players))
}

async fn get_game_state(
    Path(room): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Get active players
    let players = sqlx::query(
        "SELECT username, x, y, room, health, resources FROM players WHERE room = $1 AND last_seen > NOW() - INTERVAL '5 minutes'"
    )
    .bind(&room)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        warn!("Database error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let game_players: Vec<Player> = players
        .into_iter()
        .map(|row| Player {
            username: row.get("username"),
            x: row.get::<i32, _>("x") as u32,
            y: row.get::<i32, _>("y") as u32,
            room: row.get("room"),
            health: row.get::<i32, _>("health") as u32,
            resources: row.get::<i32, _>("resources") as u32,
        })
        .collect();

    let response = serde_json::json!({
        "room": room,
        "grid_size": GRID_SIZE,
        "players": game_players,
        "player_count": game_players.len(),
        "timestamp": chrono::Utc::now()
    });

    Ok(Json(response))
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
    let _ = state.chat_tx.send(chat_message.clone());

    Ok(Json(chat_message))
}
