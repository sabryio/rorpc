# oRPC Error Handling with Rust Backend

**Last Updated:** 2026-09-08  
**oRPC Version:** v2 beta  
**Tested With:** Rust + Axum backend, TypeScript + OpenAPILink client

## Overview

This document describes the correct way to implement type-safe error handling between a Rust backend and TypeScript frontend using oRPC v2. This pattern uses `thiserror` with `#[from]` for automatic error conversion and ensures OpenAPILink properly parses errors.

---

## Backend Implementation (Rust)

### 1. Define Error Types with `thiserror`

```rust
use serde::Serialize;
use thiserror::Error;

// Simple string error
#[derive(Debug, thiserror::Error, Serialize)]
#[error("store error: {0}")]
struct StoreError(String);

// Structured error with fields
#[derive(Debug, thiserror::Error, Serialize)]
#[error("rate limit exceeded")]
struct RateLimitError {
    #[serde(rename = "retryAfter")]  // camelCase for TypeScript
    retry_after: u64,
}
```

### 2. Create Main Error Enum with `#[from]`

```rust
use axum::http::StatusCode;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", content = "message")]  // Not used in response, but good for logging
enum RpcError {
    #[error("not found: {0}")]
    #[serde(rename = "NOT_FOUND")]
    NotFound(String),
    
    #[error("bad request: {0}")]
    #[serde(rename = "BAD_REQUEST")]
    BadRequest(String),
    
    #[error("store error: {0}")]
    #[serde(rename = "STORE_ERROR")]
    StoreError(#[from] StoreError),  // ✅ Automatic conversion
    
    #[error("rate limit exceeded")]
    #[serde(rename = "RATE_LIMIT_EXCEEDED")]
    RateLimitExceeded(#[from] RateLimitError),  // ✅ Automatic conversion
}
```

### 3. Implement `IntoResponse` for Axum

**⚠️ CRITICAL:** The response format MUST include `"defined": true` for OpenAPILink to recognize it as a typed error.

```rust
use axum::{response::IntoResponse, Json};
use serde_json::json;

impl IntoResponse for RpcError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, data) = match &self {
            RpcError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                serde_json::json!(msg),
            ),
            RpcError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                serde_json::json!(msg),
            ),
            RpcError::StoreError(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORE_ERROR",
                serde_json::json!(err.0),  // Extract inner String
            ),
            RpcError::RateLimitExceeded(err) => (
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMIT_EXCEEDED",
                serde_json::to_value(err).unwrap(),  // Serialize whole struct
            ),
        };

        // ✅ CORRECT oRPC OpenAPI error format
        let error_response = json!({
            "defined": true,      // ← REQUIRED for OpenAPILink
            "code": code,
            "message": self.to_string(),
            "data": data
        });

        (status, Json(error_response)).into_response()
    }
}
```

### 4. Use in Handlers

```rust
#[rorpc::get("/api/planets/{id}")]
async fn find_planet(
    Path(id): Path<i32>,
) -> Result<Json<Planet>, RpcError> {
    // Automatic conversion with #[from]
    if id == 999 {
        return Err(StoreError("Failed to connect to database".into()).into());
    }
    
    // Direct variant construction
    if id > 1000 {
        return Err(RpcError::NotFound(format!("Planet {} not found", id)));
    }
    
    Ok(Json(get_planet(id)))
}
```

---

## Frontend Implementation (TypeScript)

### 1. Define Contract with Error Schemas

```typescript
import { oc } from "@orpc/contract";
import { openapi } from "@orpc/openapi";
import { z } from "zod";

export const contract = {
  planet: {
    find: oc
      .meta(openapi({ method: "POST", path: "/planet/find" }))
      .errors({ 
        NOT_FOUND: { data: z.string() },           // ✅ Simple string data
        STORE_ERROR: { data: z.string() }          // ✅ Simple string data
      })
      .input(z.object({ id: z.number() }))
      .output(PlanetSchema),
  },

  ping: oc
    .meta(openapi({ method: "POST", path: "/ping" }))
    .errors({
      RATE_LIMIT_EXCEEDED: {
        data: z.object({ retryAfter: z.number() })  // ✅ Structured data
      }
    })
    .output(z.string()),
} as const;
```

### 2. Create OpenAPI Client

```typescript
import { createORPCClient } from "@orpc/client";
import { OpenAPILink } from "@orpc/openapi/fetch";

const link = new OpenAPILink(contract, {
  origin: "http://localhost:3001",
  url: "/rpc",
  fetch(url, init) {
    return globalThis.fetch(url, {
      ...init,
      credentials: "include",
    });
  },
});

export const client = createORPCClient(link);
```

### 3. Handle Errors in Components

```typescript
import { ORPCError } from "@orpc/client";

function PlanetFinder() {
  const { data, error } = useQuery(
    orpc.planet.find.queryOptions({ input: { id: 999 } })
  );

  if (error instanceof ORPCError) {
    // ✅ Type-safe error handling
    if (error.code === "STORE_ERROR") {
      console.log("Store error:", error.data);  // string
    }
    
    if (error.code === "RATE_LIMIT_EXCEEDED") {
      // ✅ TypeScript knows data is { retryAfter: number }
      console.log(`Retry after ${error.data.retryAfter} seconds`);
    }
  }

  return <div>{/* ... */}</div>;
}
```

---

## Error Response Format Reference

### Backend JSON Response

**HTTP Status Code:** 4xx or 5xx (≥ 400)

**Content-Type:** `application/json`

**Body:**
```json
{
  "defined": true,
  "code": "STORE_ERROR",
  "message": "store error: store error: Failed to connect to database",
  "data": "Failed to connect to database"
}
```

For structured data:
```json
{
  "defined": true,
  "code": "RATE_LIMIT_EXCEEDED",
  "message": "rate limit exceeded",
  "data": {
    "retryAfter": 60
  }
}
```

### Field Explanations

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `defined` | `boolean` | **YES** | Must be `true` for OpenAPILink to recognize as typed error. Without this, you get `MALFORMED_ORPC_RESPONSE`. |
| `code` | `string` | **YES** | Error code matching the contract definition (e.g., `"STORE_ERROR"`). |
| `message` | `string` | No | Human-readable error message. Usually from `Display` trait (`self.to_string()`). |
| `data` | `any` | No | Error-specific data matching the Zod schema in the contract. |

---

## Error Data Schema for Custom Types

When `#[derive(OrpcError)]` processes an error enum, it calls `zod_schema_for_type()` on each variant's inner type. It can only see the **type name** of the field — it cannot look inside a custom struct's definition.

### Primitive types → precise inline schema

```rust
#[derive(OrpcError)]
enum ApiError {
    NotFound(String),   // → data: z.string()
    TooMany(u32),       // → data: z.number().int()
    Invalid(bool),      // → data: z.boolean()
}
```

### Custom struct types without `#[derive(ZodTs)]` → `z.unknown()`

The macro cannot introspect the fields of a type it doesn't own at derive time.
It emits `z.unknown()` — always valid TypeScript, always compiles.

```rust
struct StoreError(String);  // no ZodTs — fields invisible to macro

#[derive(OrpcError)]
enum ApiError {
    Store(#[from] StoreError),  // → data: z.unknown()
    Glue(#[from] GlueError),    // → data: z.unknown()
}
```

Generated contract:
```typescript
.errors({
  STORE: { data: z.unknown() },  // ✅ valid TypeScript, always compiles
  GLUE:  { data: z.unknown() },
})
```

To get a precise schema, add `#[derive(ZodTs)]` to the inner type (see next section).

### Custom struct types WITH `#[derive(ZodTs)]` → precise schema reference

```rust
#[derive(ZodTs, Serialize)]
struct RateLimitError {
    #[serde(rename = "retryAfter")]
    retry_after: u64,
}

#[derive(OrpcError)]
enum ApiError {
    RateLimit(#[from] RateLimitError),  // → data: RateLimitErrorSchema
}
```

Generated contract:
```typescript
export const RateLimitErrorSchema = z.object({ retryAfter: z.number().int() });

.errors({
  RATE_LIMIT: { data: RateLimitErrorSchema },  // ✅ precise + reusable
})
```

### Named-field enum variants → inline object schema (no separate type needed)

```rust
#[derive(OrpcError)]
enum ApiError {
    RateLimited { retry_after: u64, limit: u32 },
    // → data: z.object({ retry_after: z.number().int(), limit: z.number().int() })
}
```

---

## Common Pitfalls

### ❌ Missing `"defined": true`
```json
{
  "code": "STORE_ERROR",
  "message": "...",
  "data": "..."
}
```
**Result:** Frontend shows `MALFORMED_ORPC_RESPONSE` error.

### ❌ Using RPC Protocol Format for OpenAPI
```json
{
  "json": {
    "defined": true,
    "code": "STORE_ERROR",
    "message": "...",
    "data": "..."
  },
  "meta": []
}
```
**Result:** Frontend shows `MALFORMED_ORPC_RESPONSE` error.  
**Note:** This format is for `RPCLink`, not `OpenAPILink`.

### ❌ Wrong HTTP Status Code
```rust
StatusCode::OK  // 200
```
**Result:** Error not recognized. Must use 4xx or 5xx status codes.

### ❌ Mismatched Data Schema
**Backend:**
```rust
json!({ "retry_after": 60 })
```
**Frontend:**
```typescript
data: z.object({ retryAfter: z.number() })
```
**Result:** Validation error. Use `#[serde(rename = "retryAfter")]` in Rust.

---

## Testing Errors

### Create a Test Route

See `examples/axum-react/client/src/routes/error-test.tsx` for a complete testing page that:
- Triggers different error types
- Displays error code, message, and data
- Shows full JSON structure for debugging
- Validates type safety with TypeScript

### Test Cases

1. **Store Error** (ID: 999)
   - Backend returns `StoreError` → `RpcError::StoreError` (via `#[from]`)
   - Frontend receives `STORE_ERROR` with string data

2. **Rate Limit Error** (Random 30%)
   - Backend returns `RateLimitError` → `RpcError::RateLimitExceeded` (via `#[from]`)
   - Frontend receives `RATE_LIMIT_EXCEEDED` with `{ retryAfter: 60 }`

3. **Not Found Error** (ID: 9999)
   - Backend returns `RpcError::NotFound` directly
   - Frontend receives `NOT_FOUND` with error message string

---

## Debugging Tips

### Enable Console Logging

```typescript
const link = new OpenAPILink(contract, {
  fetch(url, init) {
    console.log("Request:", { url, init });
    return globalThis.fetch(url, init).then(res => {
      console.log("Response:", res.status, res.headers);
      return res;
    });
  },
});
```

### Log Full Error Object

```typescript
if (error) {
  console.log("Full error:", JSON.stringify(error, null, 2));
}
```

### Check Response in Network Tab

Look for:
- HTTP status code (should be ≥ 400)
- `Content-Type: application/json`
- Response body has `"defined": true`

---

## Resources

- [oRPC v2 Error Handling](https://orpc.dev/docs/error-handling)
- [oRPC v2 Migration Guide](https://orpc.dev/docs/migrations/from-v1)
- [OpenAPI Link Documentation](https://orpc.dev/docs/openapi/link)
- [OpenAPI Handler Documentation](https://orpc.dev/docs/openapi/handler)
- [thiserror crate](https://docs.rs/thiserror)

---

## Summary

**Key Requirements:**
1. ✅ Response must include `"defined": true`
2. ✅ HTTP status must be ≥ 400
3. ✅ Error code must match contract definition
4. ✅ Data schema must match Zod schema in contract
5. ✅ Use `#[serde(rename = "...")]` for camelCase fields
6. ✅ Use `#[from]` for automatic error conversions

**This format works with:**
- ✅ OpenAPILink (not RPCLink)
- ✅ Axum handlers (without OpenAPIHandler)
- ✅ Type-safe error handling on frontend
- ✅ Structured error data with full TypeScript types
