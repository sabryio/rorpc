---
name: rorpc
description: >
  Use when the user wants to build Axum HTTP handlers with automatic TypeScript contract generation,
  mentions rorpc/orpc/rpc contracts, asks about type-safe Rust-TypeScript integration,
  wants to add endpoints to an existing rorpc project, needs help with #[rorpc] macros,
  or asks how to generate TypeScript bindings from Rust handlers.
disable-model-invocation: false
---

# rorpc — Type-Safe Rust-TypeScript RPC

You are building HTTP handlers using the **rorpc** framework — annotate plain Axum handlers
with method-specific macros and get automatic TypeScript contract generation with zero runtime overhead.

## Core Pattern

```rust
use axum::{extract::State, Json};
use rorpc::ZodTs;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, ZodTs)]
pub struct Planet {
    pub id: i32,
    #[zod(min_length(1), max_length(100))]
    pub name: String,
    pub description: Option<String>,
}

#[rorpc::get("/planet/list")]
pub async fn list_planets(State(db): State<AppState>) -> Result<Json<Vec<Planet>>, AppError> {
    db.planet_repo.list().await.map(Json).map_err(AppError::from)
}

#[rorpc::contract]
#[tokio::main]
async fn main() {
    let app = rorpc::router!(state);
    axum::serve(listener, app).await.unwrap();
}
```

The handler remains a standard Axum handler. The `#[rorpc]` macro registers metadata at link time.
The `router!()` macro auto-discovers all annotated handlers. The `#[contract]` attribute generates
TypeScript bindings in debug builds.

## Method-Specific Macros (Primary Syntax)

Use these for all handlers:

```rust
// GET — no input, returns list
#[rorpc::get("/planet/list")]
async fn list_planets(State(s): State<AppState>) -> Result<Json<Vec<Planet>>, AppError>

// GET — with path parameter + query parameters, auto-merged in TypeScript contract
#[rorpc::get("/planet/{id}")]
async fn find_planet(
    State(s): State<AppState>,
    Path(id): Path<i32>,
    Query(q): Query<FindPlanetQuery>,
) -> Result<Json<Planet>, AppError>

// POST — with JSON body (Json<T> extractor)
#[rorpc::post("/planet")]
async fn create_planet(
    State(s): State<AppState>,
    Json(input): Json<CreatePlanetInput>,
) -> Result<Json<Planet>, AppError>

// DELETE — with path parameter
#[rorpc::delete("/planet/{id}")]
async fn delete_planet(
    State(s): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<()>, AppError>

// SSE streaming — data attribute for type-safe events
#[rorpc::get("/stream", data = "StreamEvent")]
pub async fn stream_events() -> Sse<impl Stream<Item = Event>>
```

**Available methods:** `get`, `post`, `put`, `patch`, `delete`

## Namespace Grouping

Group related handlers under a common path prefix:

```rust
// planet.rs
#[rorpc::namespace("/planet")]
pub mod routes {
    use super::*;

    #[rorpc::get("/list")]           // Becomes /planet/list
    pub async fn list(State(s): State<AppState>) -> Result<Json<Vec<Planet>>, AppError> {
        // ...
    }

    #[rorpc::get("/{id}")]           // Becomes /planet/{id}
    pub async fn find(
        State(s): State<AppState>,
        Path(id): Path<i32>,
    ) -> Result<Json<Planet>, AppError> {
        // ...
    }

    #[rorpc::post("/")]              // Becomes /planet
    pub async fn create(
        State(s): State<AppState>,
        Json(input): Json<CreatePlanetInput>,
    ) -> Result<Json<Planet>, AppError> {
        // ...
    }
}
```

**Rules:**
- Prefix must start with `/`
- Prefix cannot contain `..` path traversal
- Namespace always concatenates with handler path: `/api` + `/planet/list` = `/api/planet/list`
- **Must use inline module** (`pub mod routes { ... }`) — file modules don't work on stable Rust

**Nested namespaces for versioning:**
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

## Request Body vs Query Parameters

The oRPC OpenAPILink client automatically handles the difference:

- **GET handlers:** Use `Query<T>` extractor → data sent as URL query parameters
- **POST/PUT/PATCH/DELETE:** Use `Json<T>` extractor → data sent as JSON request body
- **Path parameters:** Use `Path<T>` extractor → extracted from URL path segments (e.g., `{id}`)

When `Path<T>` and `Query<T>` are both present, they are **automatically merged** in the TypeScript contract:
```typescript
.input(z.object({ id: z.number().int() }).extend(QuerySchema.shape))
```

All render as `.input()` in the TypeScript contract.

## Type-Safe Schemas with ZodTs

```rust
use rorpc::ZodTs;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, ZodTs)]
pub struct Planet {
    pub id: i32,
    #[zod(min_length(1), max_length(100))]
    pub name: String,
    pub description: Option<String>,
    #[zod(email)]
    pub contact_email: Option<String>,
}
```

**Supported `#[zod(...)]` constraints:**

- **Strings:** `min_length(n)`, `max_length(n)`, `length(n)`, `email`, `url`, `regex("pattern")`,
  `starts_with("s")`, `ends_with("s")`, `includes("s")`
- **Numbers:** `min(n)`, `max(n)`, `int`, `positive`, `negative`, `nonnegative`, `nonpositive`, `finite`
- **Arrays:** `min_length(n)`, `max_length(n)`, `length(n)`

## Error Schemas

```rust
use rorpc::OrpcError;

#[derive(OrpcError)]
pub enum AppError {
    NotFound,                    // → NOT_FOUND: {}
    Unauthorized,                // → UNAUTHORIZED: {}
    BadRequest { reason: String },  // → BAD_REQUEST: { data: z.object({ reason: z.string() }) }
    Internal(String),            // → INTERNAL: { data: z.string() }
}
```

The derive macro converts variant names to `SCREAMING_SNAKE_CASE` and generates TypeScript `.errors({...})` schemas.

## Contract Generation

### Option 1: `#[contract]` Attribute (Recommended)

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

**Only runs in debug builds** (`#[cfg(debug_assertions)]`) — release builds don't generate files.

### Option 2: Explicit Call

```rust
rorpc::generate_contract()
    .output(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../client/src/rpc/bindings.ts"
    ))
    .expect("contract generation failed");
```

Use this when you need conditional generation logic beyond debug/release.

## Router Discovery

```rust
use axum::Router;
use rorpc::router;

// All handlers + state
let app = router!(state);

// Nested under a path prefix (common pattern)
let app = Router::new()
    .nest("/rpc", rorpc::router!(state));

// Module filtering
let app = router!("handlers::planet", state);
let app = router!("handlers::{planet,user}");  // brace expansion
let app = router!("handlers::*");              // wildcard
let app = router!(["handlers::planet", "api::v1"], state);
```

The `router!()` macro uses the `inventory` crate to collect all `HandlerRegistration` entries at link time.
No central list required, no startup registry call — all discovery happens at compile time.

**Common pattern:** Use `.nest("/rpc", rorpc::router!(state))` to mount all rorpc handlers under a common prefix like `/rpc`, keeping them separate from other routes in your application.

## Generated TypeScript Contract

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
      .input(z.object({ id: z.number().int() }).extend(FindPlanetQuerySchema.shape))
      .output(PlanetSchema)
      .errors({
        NOT_FOUND: {},
        INTERNAL: { data: z.object({ msg: z.string() }) },
      }),
    createPlanet: oc
      .meta(openapi({ method: "POST", path: "/planet" }))
      .input(CreatePlanetInputSchema)
      .output(PlanetSchema),
  },
} as const;
```

## TypeScript Client Usage

```typescript
import { createORPCClient } from "@orpc/client";
import { OpenAPILink } from "@orpc/openapi/fetch";
import { createTanstackQueryUtils } from "@orpc/tanstack-query";
import { contract } from "./bindings";

const link = new OpenAPILink(contract, {
  origin: "http://localhost:3001",
  url: "/rpc",
  fetch: (url, init) => fetch(url, { ...init, credentials: "include" }),
});

export const client = createORPCClient(link);
export const orpc = createTanstackQueryUtils(client);
```

```tsx
import { orpc } from "@/rpc/contract";
import { useQuery, useMutation } from "@tanstack/react-query";

// Query
const { data } = useQuery(orpc.planet.listPlanets.queryOptions());

// Mutation with cache invalidation
const mutation = useMutation(
  orpc.planet.createPlanet.mutationOptions({
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: orpc.planet.key() }),
  }),
);

// Delete
const deleteMutation = useMutation(
  orpc.planet.deletePlanet.mutationOptions({
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: orpc.planet.key() }),
  }),
);
deleteMutation.mutate({ id: 1 });

// Direct call (outside React)
const planet = await orpc.planet.findPlanet.call({ id: 1 });
```

## Architecture

rorpc is split into three crates internally:

- **`rorpc`** — Main crate with runtime types, contract generation, and macro re-exports
- **`rorpc-macros`** — Proc-macro bridge (thin entry points)
- **`rorpc-parse`** — All AST parsing and codegen logic (testable with normal `#[test]`)

You only need to add `rorpc` to your dependencies — it re-exports everything you need.

## How It Works

Understanding the key patterns helps you use rorpc effectively:

1. **Handlers remain pure Axum** — No wrapper types or special layers. The `#[rorpc]` macro
   returns the original function unchanged, only registering metadata via `inventory`.

2. **Link-time discovery** — The `inventory` crate collects handler metadata at link time.
   Zero runtime overhead, no startup registry call needed.

3. **Path parameter merging** — When `Path<T>` and `Query<T>` are both present, the TypeScript
   contract automatically merges them: `z.object({ id: z.number().int() }).extend(QuerySchema.shape)`.

4. **Namespace concatenation** — Namespace prefix always concatenates with handler path.
   `/api` + `/planet/list` = `/api/planet/list`. Never replaces, always prepends.

5. **SSE data attribute** — Streaming handlers use `data = "TypeName"` string literal syntax
   for IDE autocomplete support.

6. **Contract generation in debug only** — The `#[contract]` attribute wraps generation in
   `#[cfg(debug_assertions)]`. Release builds never generate files.

7. **OpenAPILink, not RPCLink** — TypeScript clients use `OpenAPILink` from `@orpc/openapi/fetch`,
   not `RPCLink`. Rust/Axum speaks plain JSON, not oRPC wire envelope protocol.

## Common Mistakes to Avoid

1. **Don't use file modules with `#[namespace]`** — Must be inline `pub mod routes { ... }`.
   File modules (`#[namespace] mod planet;`) don't work on stable Rust.

2. **Don't manually collect handlers** — The `router!()` macro auto-discovers all handlers via
   `inventory`. No central list needed.

3. **Don't call `generate_contract()` in main when using `#[contract]`** — The attribute
   already does this. Duplicate calls will generate twice.

4. **Don't forget `Json<T>` wrapper for POST bodies** — Use `Json<T>` extractor for request bodies
   on POST/PUT/PATCH handlers, and return `Json<T>` for responses.

## When to Use Which Syntax

- **Method-specific macros** (`#[rorpc::get]`, etc.) — Primary syntax, use for all handlers
- **`#[rorpc::route]`** — Only when you need explicit method/path or non-standard HTTP methods
- **`#[rorpc::namespace]`** — When multiple handlers share a common path prefix
- **`#[contract]` attribute** — Preferred over manual `generate_contract()` calls
- **`router!()` with filters** — Only when you need module-based inclusion/exclusion

## Installation

Add rorpc to your `Cargo.toml`:

```toml
[dependencies]
rorpc = "0.1"
axum = "0.8"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

Configure TypeScript contract output path:

```toml
[package.metadata.rorpc]
client_path = "../client/src/rpc/bindings.ts"
```

## Dependencies

| Crate          | Version | Key dependencies                                      |
| -------------- | ------- | ----------------------------------------------------- |
| `rorpc`        | `0.1`   | `axum 0.8`, `inventory 0.3`, `serde 1.0`              |
| `rorpc-macros` | `0.1`   | `syn 3.0`, `proc-macro2 1.0`                          |
| `rorpc-parse`  | `0.1`   | `syn 3.0` (full + extra-traits), `quote 1.0`, `inventory 0.3` |
