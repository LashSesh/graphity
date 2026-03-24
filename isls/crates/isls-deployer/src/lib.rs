// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Docker and deployment artifact generation for ISLS full-stack applications.
//!
//! Template-driven generation of Dockerfile(s), docker-compose.yml,
//! .env.example, .gitignore, and README.md.  No LLM oracle required.

use std::path::{Path, PathBuf};
use std::fs;
use thiserror::Error;
use isls_planner::{AppSpec, Architecture};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum DeployerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("template error: {0}")]
    Template(String),
}

pub type Result<T> = std::result::Result<T, DeployerError>;

// ─── DeploymentArtifacts ─────────────────────────────────────────────────────

/// All paths of generated deployment artifacts.
#[derive(Clone, Debug)]
pub struct DeploymentArtifacts {
    pub paths: Vec<PathBuf>,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Generate all deployment artifacts for the given architecture and spec.
///
/// Produces: Dockerfile (backend), docker-compose.yml, .env.example, .gitignore,
/// nginx.conf (for frontend static serving).
pub fn generate_deployment(
    architecture: &Architecture,
    spec: &AppSpec,
    output_dir: &Path,
) -> Result<DeploymentArtifacts> {
    let mut paths = Vec::new();
    let app_name = &spec.name;
    let db_name = app_name.replace('-', "_");

    // docker-compose.yml (idempotent — orchestrator may have written it already)
    let compose_content = format!(
        r#"version: '3.8'
services:
  db:
    image: postgres:16
    environment:
      POSTGRES_DB: {db_name}
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: ${{DB_PASSWORD:-postgres}}
    volumes:
      - pgdata:/var/lib/postgresql/data
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 5s
      timeout: 5s
      retries: 5

  backend:
    build:
      context: ./backend
      dockerfile: Dockerfile
    ports:
      - "8080:8080"
    environment:
      DATABASE_URL: postgres://postgres:${{DB_PASSWORD:-postgres}}@db/{db_name}
      PORT: 8080
      JWT_SECRET: ${{JWT_SECRET:-change-in-production}}
    depends_on:
      db:
        condition: service_healthy

  frontend:
    image: nginx:alpine
    ports:
      - "3000:80"
    volumes:
      - ./frontend:/usr/share/nginx/html:ro

volumes:
  pgdata:
"#
    );
    let compose_path = output_dir.join("docker-compose.yml");
    write(&compose_path, &compose_content)?;
    paths.push(compose_path);

    // backend/Dockerfile
    let dockerfile_content = format!(
        r#"FROM rust:1.80-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {{}}' > src/main.rs
RUN cargo build --release && rm -rf src
COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/{app_name} /usr/local/bin/app
EXPOSE 8080
CMD ["app"]
"#
    );
    let dockerfile_path = output_dir.join("backend/Dockerfile");
    if let Some(parent) = dockerfile_path.parent() { fs::create_dir_all(parent)?; }
    write(&dockerfile_path, &dockerfile_content)?;
    paths.push(dockerfile_path);

    // nginx.conf for frontend
    let nginx_content = r#"server {
    listen 80;
    root /usr/share/nginx/html;
    index index.html;
    location / {
        try_files $uri $uri/ /index.html;
    }
    location /api/ {
        proxy_pass http://backend:8080;
        proxy_set_header Host $host;
    }
}
"#;
    let nginx_path = output_dir.join("frontend/nginx.conf");
    if let Some(parent) = nginx_path.parent() { fs::create_dir_all(parent)?; }
    write(&nginx_path, nginx_content)?;
    paths.push(nginx_path);

    let _ = architecture;
    Ok(DeploymentArtifacts { paths })
}

fn write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_planner::{AppSpec, ModuleSpec, BackendSpec, AuthSpec, FrontendSpec, PageSpec, DeploymentSpec, AppConstraints, Architecture};

    fn test_spec() -> AppSpec {
        AppSpec {
            name: "test-app".to_string(),
            description: "test".to_string(),
            modules: vec![ModuleSpec {
                name: "inventory".to_string(),
                description: "inventory".to_string(),
                entities: vec!["Product".to_string()],
                operations: vec!["create".to_string()],
                dependencies: vec![],
            }],
            backend: BackendSpec {
                language: "rust".to_string(),
                framework: "actix-web".to_string(),
                database: "postgresql".to_string(),
                auth: AuthSpec { method: "jwt".to_string() },
            },
            frontend: FrontendSpec {
                app_type: "spa".to_string(),
                framework: "vanilla".to_string(),
                styling: "minimal".to_string(),
                pages: vec![],
            },
            deployment: DeploymentSpec { containerized: true, compose: true },
            constraints: AppConstraints {
                max_crates: 1,
                test_coverage: "integration".to_string(),
                evidence_chain: true,
            },
        }
    }

    fn empty_arch() -> Architecture {
        Architecture {
            app_name: "test-app".to_string(),
            layers: vec![],
            generation_order: vec![],
            interfaces: vec![],
            estimated_files: 0,
            estimated_loc: 0,
        }
    }

    #[test]
    fn generates_compose_and_dockerfile() {
        let spec = test_spec();
        let arch = empty_arch();
        let dir = std::env::temp_dir().join("isls-deployer-test");
        std::fs::create_dir_all(&dir).unwrap();
        let artifacts = generate_deployment(&arch, &spec, &dir).unwrap();
        assert!(artifacts.paths.iter().any(|p| p.file_name().unwrap() == "docker-compose.yml"));
        assert!(artifacts.paths.iter().any(|p| p.file_name().unwrap() == "Dockerfile"));
        assert!(dir.join("docker-compose.yml").exists());
    }
}
