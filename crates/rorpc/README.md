# rorpc

[![Crates.io](https://img.shields.io/crates/v/rorpc.svg)](https://crates.io/crates/rorpc)
[![Documentation](https://docs.rs/rorpc/badge.svg)](https://docs.rs/rorpc)
[![License](https://img.shields.io/crates/l/rorpc.svg)](https://github.com/sabryio/rorpc)

Rust oRPC — annotation-driven Axum handlers with automatic TypeScript contract generation.

## Overview

`rorpc` bridges Rust Axum handlers with TypeScript clients through compile-time metadata collection and automatic code generation. Annotate handlers with method-specific macros (`#[rorpc::get]`, `#[rorpc::post]`, etc.), derive Zod schemas, and generate type-safe TypeScript contracts with zero runtime overhead.

## Features

- **Zero boilerplate routing** — Auto-discover handlers with `router!()`
- **Type-safe contracts** — Generate TypeScript Zod schemas from Rust types
- **Compile-time registration** — Uses `inventory` crate for link-time collection
- **Streaming support** — SSE endpoints with `Sse<impl Stream>`
- **Error schemas** — TypeScript error enums from Rust error types
- **Module filtering** — Include/exclude handlers by module path

## Quick Start

```rust
use axum::{extract::State, Json};
use rorpc::{router, ZodTs};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, ZodTs)]
pub struct Planet {
    pub id: i32,
    #[zod(min_length(1), max_length(100))]
    pub name: String,
    pub description: Option<String>,
}

#[rorpc::post("/planet/list")]
async fn list_planets(State(db): State<Database>) -> Json<Vec<Planet>> {
    Json(db.list().await)
}

#[rorpc::post("/planet/find")]
async fn find_planet(
    State(db): State<Database>,
    Json(id): Json<i32>,
) -> Result<Json<Planet>, AppError> {
    db.find(id).await.map(Json).ok_or(AppError::NotFound)
}

#[rorpc::contract]
#[tokio::main]
async fn main() {
    let app = router!(state);
    axum::serve(listener, app).await.unwrap();
}
```

Configure the output path in `Cargo.toml`:
```toml
[package.metadata.rorpc]
client_path = "../client/src/rpc/index.ts"
```

## Macros

### `#[contract]` — Automatic Contract Generation (New!)

Replaces manual `generate_contract().output()` boilerplate. Automatically generates TypeScript contracts in debug builds before `main()` runs.

Configure the output path in `Cargo.toml`:

```toml
[package.metadata.rorpc]
client_path = "../client/src/rpc/bindings.ts"
```

```rust
#[rorpc::contract]
#[tokio::main]
async fn main() {
    let app = router!(state);
    axum::serve(listener, app).await.unwrap();
}
```

**Options:**
- `#[contract]` — reads `[package.metadata.rorpc] client_path` from `Cargo.toml`
- `#[contract("../client/bindings.ts")]` — string literal
- `#[contract(CLIENT_PATH)]` — constant
- `#[contract(concat!(...))]` — expression

See [docs/metadata-bridge.md](../../docs/metadata-bridge.md) for more options.

### Method-Specific Shorthands (Recommended)

```rust
// GET request
#[rorpc::get("/ping")]
async fn ping(State(s): State<AppState>) -> Json<String>

// POST with JSON input
#[rorpc::post("/create")]
async fn create(State(db): State<Db>, Json(data): Json<CreateInput>) -> Json<Planet>

// With Result for typed errors
#[rorpc::post("/find")]
async fn find(State(db): State<Db>, Json(id): Json<i32>) -> Result<Json<Planet>, AppError>

// GET with path parameter + query — auto-merged in TypeScript contract
#[rorpc::get("/planet/{id}")]
async fn find_planet(
    State(db): State<Db>,
    Path(id): Path<i32>,
    Query(q): Query<FindQuery>,
) -> Result<Json<Planet>, AppError>

// Streaming (SSE) with data type annotation
#[rorpc::get("/events", data = "StreamEvent")]
async fn stream() -> Sse<impl Stream<Item = Event>>
```

**Available methods:** `get`, `post`, `put`, `patch`, `delete`

### `#[rorpc::namespace]` — Path Grouping

Group related handlers under a common path prefix using an inline module:

```rust
// planet.rs
use axum::{extract::State, Json};

#[rorpc::namespace("/planet")]
pub mod routes {
    use super::*;

    #[rorpc::get("/list")]           // Becomes /planet/list
    pub async fn list(State(db): State<Db>) -> Json<Vec<Planet>> {
        Json(db.list().await)
    }

    #[rorpc::post("/")]              // Becomes /planet
    pub async fn create(
        State(db): State<Db>,
        Json(input): Json<CreateInput>,
    ) -> Result<Json<Planet>, AppError> {
        db.create(input).await.map(Json)
    }

    #[rorpc::get("/{id}")]           // Becomes /planet/{id}
    pub async fn find(State(db): State<Db>, Path(id): Path<i32>) -> Result<Json<Planet>, AppError> {
        db.find(id).await.map(Json).ok_or(AppError::NotFound)
    }
}
```

**Benefits:**
- DRY: No repeated `/planet/` prefix in every handler
- Easier refactoring: Change prefix in one place
- Clear module organization: Group by domain/resource

**Why inline modules?**

Due to Rust's proc macro stability limitations, `#[namespace]` requires an inline module (`pub mod routes { ... }`). Neither file modules (`#[namespace] mod planet;`) nor inner attributes (`#![namespace("/planet")]`) work on stable Rust.

**Nested namespaces** for versioning:
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

### `#[rorpc::route]` — Explicit Method + Path

For non-standard methods or when you prefer explicit syntax:

```rust
#[rorpc::route(method = "GET", path = "/planet/list")]
async fn list_planets(State(s): State<AppState>) -> Json<Vec<Planet>>
```

**Required attributes:** `method`, `path`  
**Optional attributes:** `data` (SSE handlers)

### `router!`

Auto-discover all annotated handlers and build an Axum `Router`.

```rust
let app = router!();                            // all handlers, no state
let app = router!(state);                       // with state
let app = router!("handlers::planet");          // module filter
let app = router!("handlers::*");              // wildcard
let app = router!("handlers::{planet,user}");  // brace expansion
let app = router!(["handlers::planet"], state); // filter + state
```

### `#[derive(ZodTs)]`

Generate TypeScript Zod schemas from Rust types.

```rust
#[derive(Serialize, Deserialize, ZodTs)]
pub struct User {
    pub id: i32,
    #[zod(email)]
    pub email: String,
    #[zod(min_length(8))]
    pub password: String,
    #[zod(min(13), max(120))]
    pub age: Option<i32>,
}
```

### `#[derive(OrpcError)]`

Generate TypeScript error schemas from Rust error enums.

```rust
#[derive(OrpcError)]
pub enum AppError {
    NotFound,                    // → NOT_FOUND: {}
    Unauthorized,                // → UNAUTHORIZED: {}
    Conflict { reason: String }, // → CONFLICT: { data: z.object({ reason: z.string() }) }
    DatabaseError(String),       // → DATABASE_ERROR: { data: z.string() }
}
```

## Contract Generation

```rust
rorpc::generate_contract()
    .output("../client/src/rpc/index.ts")
    .unwrap();
```

Generated TypeScript:

```typescript
// AUTO-GENERATED by rorpc — do not edit manually.

import { z } from "zod";
import { oc } from "@orpc/contract";
import { openapi } from "@orpc/openapi";

export const PlanetSchema = z.object({
  id: z.number().int(),
  name: z.string().min(1).max(100),
  description: z.string().optional(),
});
export type Planet = z.infer<typeof PlanetSchema>;

export const contract = {
  planet: {
    listPlanets: oc
      .meta(openapi({ method: "GET", path: "/planet/list" }))
      .output(z.array(PlanetSchema)),
    findPlanet: oc
      .meta(openapi({ method: "GET", path: "/planet/{id}" }))
      .input(z.object({ id: z.number().int() }).extend(FindQuerySchema.shape))
      .output(PlanetSchema)
      .errors({ NOT_FOUND: {}, UNAUTHORIZED: {} }),
  },
} as const;

export type Contract = typeof contract;
```

## Architecture

```
rorpc (runtime + facade)
  ├── rorpc-macros (proc-macro bridge)
  │     └── rorpc-parse (parsing + codegen)
  │           └── Shared utilities used by both compile-time and runtime
  └── dependencies:
        ├── inventory (compile-time registration)
        ├── axum (web framework)
        └── serde (serialization)
```

**Shared parsing utilities:** The `rorpc` runtime crate reuses type-to-Zod conversion logic from `rorpc-parse`, ensuring consistency between compile-time macro expansion and runtime contract generation.

### Module Structure

- **`metadata`** — `HandlerMetadata` collected by `#[rorpc]` for contract generation
- **`registration`** — `HandlerRegistration` with type-erased route factories for `router!()`
- **`schema_registry`** — `SchemaRegistration` for Zod schemas from `#[derive(ZodTs)]`
- **`error_registry`** — `ErrorRegistration` for error enums from `#[derive(OrpcErrors)]`
- **`codegen`** — TypeScript contract generation (`ContractBuilder`)

## Installation

```toml
[dependencies]
rorpc = "0.1"
axum = "0.8"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

## Examples

See `examples/` directory:

- `examples/axum-react/server` — Basic example
- `examples/axum-react/better-auth-integration` — Auth integration with Better Auth

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

at your option.
