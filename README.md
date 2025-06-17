# Server Application

A Rust backend server that handles client requests and manages PostgreSQL database operations.

## Features

- RESTful API endpoints
- PostgreSQL database integration
- Connection pooling and management
- Error handling and logging
- Authentication and authorization
- Railway deployment ready

## Getting Started

### Prerequisites

- Rust (latest stable version)
- Cargo package manager
- PostgreSQL database access

### Installation

1. Navigate to the server directory:
   ```bash
   cd server
   ```

2. Build the project:
   ```bash
   cargo build
   ```

3. Run the server:
   ```bash
   cargo run
   ```

## Configuration

### Database Connection

The server connects to the Railway PostgreSQL database:

- **Production**: `crossover.proxy.rlwy.net:57987`
- **Internal**: `postgres.railway.internal:5432`
- **Database**: `clever-youth`

### Environment Variables

Create a `.env` file in the server directory with:

```env
DATABASE_URL=postgresql://username:password@crossover.proxy.rlwy.net:57987/railway
PORT=8080
RUST_LOG=info
MAX_CONNECTIONS=10
```

## API Endpoints

The server provides the following endpoints:

- `GET /health` - Health check endpoint
- `GET /api/data` - Retrieve data from database
- `POST /api/data` - Create new data entries
- `PUT /api/data/:id` - Update existing data
- `DELETE /api/data/:id` - Delete data entries

## Database Schema

The server manages the following database tables:
- Users
- Data entries
- Session management
- Audit logs

## Development

### Building for Production

```bash
cargo build --release
```

### Running Tests

```bash
cargo test
```

### Database Migrations

```bash
# Add migration commands here
```

## Railway Deployment

This server is configured for Railway deployment:

1. **Dockerfile**: Production-ready containerization
2. **Environment Variables**: Configure in Railway dashboard
3. **Health Checks**: Built-in health check endpoint
4. **Logging**: Structured logging for Railway observability

### Railway Configuration

- **Port**: Server binds to `0.0.0.0:$PORT`
- **Health Check**: `/health` endpoint
- **Environment**: Production-ready with error handling
- **Database**: Automatic connection to Railway PostgreSQL

## Dependencies

Key Rust crates used:
- `tokio` - Async runtime
- `sqlx` - Database toolkit
- `serde` - Serialization/deserialization
- `axum` or `warp` - Web framework
- `tracing` - Logging

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and ensure they pass
5. Submit a pull request 