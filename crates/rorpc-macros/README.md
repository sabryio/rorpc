# rorpc-macros

[![Crates.io](https://img.shields.io/crates/v/rorpc-macros.svg)](https://crates.io/crates/rorpc-macros)
[![Documentation](https://docs.rs/rorpc-macros/badge.svg)](https://docs.rs/rorpc-macros)
[![License](https://img.shields.io/crates/l/rorpc-macros.svg)](https://github.com/sabryio/rorpc)

Procedural macro bridge for [rorpc](https://crates.io/crates/rorpc) — thin wrappers over [`rorpc-parse`](https://crates.io/crates/rorpc-parse).

## Overview

This crate contains only proc-macro entry points. All parsing, validation, and code generation logic lives in `rorpc-parse` where it can be tested with normal `#[test]` functions.

The entire implementation is a single `lib.rs` file with five proc macros that delegate to `rorpc-parse`.

## Macros

### `#[contract]`

Automatically generate TypeScript contract before `fn main()` runs (debug builds only). Replaces manual `generate_contract()` boilerplate.

Configure the output path in `Cargo.toml`:

```toml
[package.metadata.rorpc]
client_path = "../client/src/rpc/bindings.ts"
```

```rust
#[rorpc::contract]
#[tokio::main]
async fn main() {
    let app = rorpc::router!(state);
    axum::serve(listener, app).await.unwrap();
}
```

**Supported syntaxes:**
- `#[contract]` — reads `[package.metadata.rorpc] client_path` from `Cargo.toml`
- `#[contract("../client/bindings.ts")]` — string literal path
- `#[contract(CLIENT_PATH)]` — constant
- `#[contract(concat!(...))]` — concat expression

See [docs/metadata-bridge.md](../../docs/metadata-bridge.md) for setup options.

### Method-Specific Shorthands

Concise syntax for common HTTP methods:

```rust
use axum::{extract::State, Json};

#[rorpc::post("/planet/list")]
async fn list_planets(State(db): State<Db>) -> Json<Vec<Planet>> {
    Json(db.list().await)
}

#[rorpc::get("/planet/{id}")]
async fn find_planet(
    State(db): State<Db>,
    Path(id): Path<i32>,
    Query(q): Query<FindQuery>,
) -> Result<Json<Planet>, AppError> {
    db.find(id, q).await.map(Json).ok_or(AppError::NotFound)
}
```

**Available methods:** `get`, `post`, `put`, `patch`, `delete`

**Optional attributes:**

- `data` — SSE data payload type for streaming handlers (e.g. `data = "StreamEvent"`)

### `#[rorpc::namespace]`

Group related handlers under a common path prefix using an inline module:

```rust
// planet.rs

#[rorpc::namespace("/planet")]
pub mod routes {
    use super::*;

    #[rorpc::get("/list")]     // Becomes /planet/list
    pub async fn list(State(db): State<Db>) -> Json<Vec<Planet>> {
        Json(db.list().await)
    }

    #[rorpc::get("/{id}")]     // Becomes /planet/{id}
    pub async fn find(State(db): State<Db>, Path(id): Path<i32>) -> Result<Json<Planet>, AppError> {
        db.find(id).await.map(Json).ok_or(AppError::NotFound)
    }
}
```

**Rules:**
- Prefix must start with `/`
- Prefix cannot contain `..` path traversal
- Namespace concatenates with handler path: `/api` + `/status` = `/api/status`
- **Requires inline module** — file modules and inner attributes don't work on stable Rust

**Supports nested namespaces:**
```rust
#[rorpc::namespace("/api")]
pub mod api {
    #[rorpc::namespace("/v1")]
    pub mod v1 {
        use super::*;
        
        #[rorpc::get("/status")]  // Becomes /api/v1/status
        pub async fn status() -> Json<&'static str> {
            Json("ok")
        }
    }
}
```

### `#[rorpc::route]`

Explicit method + path syntax:

```rust
#[rorpc::route(method = "POST", path = "/planet/list")]
async fn list_planets(State(db): State<Db>) -> Json<Vec<Planet>> {
    Json(db.list().await)
}
```

**Required attributes:**

- `method` — HTTP method (`"GET"`, `"POST"`, etc.)
- `path` — Route path (e.g. `"/planet/list"`)

**Optional attributes:**

- `data` — SSE data payload type for streaming handlers

### `router!`

Auto-discovery macro that builds an Axum `Router` from all annotated handlers using the `inventory` crate.

```rust
use rorpc::router;

// All handlers, no state
let app = router!();

// With state
let app = router!(db);

// Module filtering
let app = router!("handlers::planet");
let app = router!(["handlers::planet", "api::v1"]);
let app = router!("handlers::{planet,user}");  // brace expansion
let app = router!("handlers::*");              // wildcard

// Filtering + state (any order)
let app = router!("handlers::planet", db);
let app = router!(db, "handlers::planet");
```

### `#[derive(ZodTs)]`

Generate a `fn zod_ts() -> String` method that returns TypeScript Zod schemas. The generated schema is registered via `inventory` for contract generation.

```rust
use rorpc::ZodTs;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, ZodTs)]
pub struct Planet {
    pub id: i32,
    #[zod(min_length(1), max_length(100))]
    pub name: String,
    pub description: Option<String>,
}
```

**Supported `#[zod(...)]` attributes:**

- **Strings:** `min_length(n)`, `max_length(n)`, `length(n)`, `email`, `url`, `regex("pattern")`, `starts_with("s")`, `ends_with("s")`, `includes("s")`
- **Numbers:** `min(n)`, `max(n)`, `int`, `positive`, `negative`, `nonnegative`, `nonpositive`, `finite`
- **Arrays:** `min_length(n)`, `max_length(n)`, `length(n)`

### `#[derive(OrpcErrors)]`

Register error enum variants for TypeScript contract generation. Variant names are converted to `SCREAMING_SNAKE_CASE`.

```rust
use rorpc::OrpcErrors;

#[derive(OrpcErrors)]
pub enum AppError {
    NotFound,                           // → NOT_FOUND: {}
    Conflict { reason: String },        // → CONFLICT: { data: z.object({...}) }
    DatabaseError(String),              // → DATABASE_ERROR: { data: z.string() }
}
```

## Installation

This crate is typically used via the `rorpc` facade crate:

```toml
[dependencies]
rorpc = "0.1"
```

Or add it directly (not recommended):

```toml
[dependencies]
rorpc-macros = "0.1"
```

## Architecture

```
rorpc-macros (proc-macro bridge, lib.rs only)
     └── rorpc-parse (all implementation, fully testable)
           └── syn 3.0, quote, proc-macro2, inventory
```

**Why the split?**

- Proc-macro crates can't have normal `#[test]` functions
- All logic in `rorpc-parse` can be unit-tested
- `rorpc-macros` is just thin `TokenStream` conversion wrappers
