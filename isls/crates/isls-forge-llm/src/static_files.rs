// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Static file generators for ISLS v3.1.
//!
//! These are simple Rust string-building functions, not Tera templates.
//! They produce files whose structure is always the same regardless of domain
//! (Cargo.toml, Dockerfile, docker-compose.yml, .env.example, .gitignore).

use crate::AppSpec;

// ─── Cargo.toml ───────────────────────────────────────────────────────────────

/// Generate a `backend/Cargo.toml` for the given application spec.
pub fn generate_cargo_toml(spec: &AppSpec) -> String {
    let name = &spec.app_name;
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{name}"
path = "src/main.rs"

[dependencies]
actix-web       = "4"
actix-cors      = "0.7"
sqlx            = {{ version = "0.7", features = ["runtime-tokio-rustls", "postgres", "chrono", "uuid"] }}
tokio           = {{ version = "1", features = ["full"] }}
serde           = {{ version = "1", features = ["derive"] }}
serde_json      = "1"
jsonwebtoken    = "9"
bcrypt          = "0.15"
chrono          = {{ version = "0.4", features = ["serde"] }}
uuid            = {{ version = "1", features = ["v4", "serde"] }}
tracing         = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}
dotenvy         = "0.15"
thiserror       = "1"
validator       = {{ version = "0.18", features = ["derive"] }}
futures         = "0.3"

[dev-dependencies]
actix-rt = "2"

[workspace]
"#,
        name = name
    )
}

// ─── docker-compose.yml ───────────────────────────────────────────────────────

/// Generate a `docker-compose.yml` for the application.
pub fn generate_docker_compose(spec: &AppSpec) -> String {
    let _name = &spec.app_name;
    let name_snake = spec.app_name_snake();
    format!(
        r#"version: "3.9"

services:
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: {name_snake}
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./backend/migrations:/docker-entrypoint-initdb.d
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres -d {name_snake}"]
      interval: 5s
      timeout: 5s
      retries: 10

  backend:
    build:
      context: ./backend
      dockerfile: Dockerfile
    ports:
      - "8080:8080"
    environment:
      DATABASE_URL: postgres://postgres:postgres@db:5432/{name_snake}
      JWT_SECRET: change-me-in-production-please
      PORT: "8080"
      RUST_LOG: info
    depends_on:
      db:
        condition: service_healthy
    healthcheck:
      test: ["CMD-SHELL", "curl -sf http://localhost:8080/api/health || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 6

  frontend:
    image: nginx:alpine
    ports:
      - "3000:80"
    volumes:
      - ./frontend:/usr/share/nginx/html:ro
      - ./frontend/nginx.conf:/etc/nginx/conf.d/default.conf:ro
    depends_on:
      - backend

volumes:
  postgres_data:
"#,
        name_snake = name_snake
    )
}

// ─── Dockerfile ───────────────────────────────────────────────────────────────

/// Generate a multi-stage Dockerfile for the Rust backend.
pub fn generate_dockerfile(spec: &AppSpec) -> String {
    let name = &spec.app_name;
    format!(
        r#"# ── Build stage ──────────────────────────────────────────────────────────────
FROM rust:1.85-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml ./
COPY migrations ./migrations
RUN mkdir src && echo 'fn main() {{}}' > src/main.rs && cargo build --release && rm src/main.rs

COPY src ./src
RUN touch src/main.rs && cargo build --release

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/{name} ./server

ENV PORT=8080
EXPOSE 8080

CMD ["./server"]
"#,
        name = name
    )
}

// ─── .env.example ─────────────────────────────────────────────────────────────

/// Generate a `.env.example` file for the application.
pub fn generate_env_example(spec: &AppSpec) -> String {
    let name_snake = spec.app_name_snake();
    format!(
        r#"# Copy this file to .env and fill in your values.
DATABASE_URL=postgres://postgres:postgres@localhost:5432/{name}
JWT_SECRET=change-me-to-a-long-random-string
PORT=8080
RUST_LOG=info,{name}=debug
"#,
        name = name_snake
    )
}

// ─── .gitignore ───────────────────────────────────────────────────────────────

/// Standard Rust + Docker .gitignore.
pub const GITIGNORE_TEMPLATE: &str = r#"/target
/out
Cargo.lock
**/*.rs.bk
.env
*.pem
*.key
__pycache__/
.DS_Store
"#;

// ─── nginx.conf ───────────────────────────────────────────────────────────────

/// Minimal nginx config for the frontend SPA.
pub const NGINX_CONF: &str = r#"server {
    listen 80;
    root /usr/share/nginx/html;
    index index.html;

    location / {
        try_files $uri $uri/ /index.html;
    }

    location /api/ {
        proxy_pass http://backend:8080/api/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
"#;
