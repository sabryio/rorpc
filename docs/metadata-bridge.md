# Configuring `#[rorpc::contract]`

The `#[rorpc::contract]` attribute automatically generates TypeScript bindings before
`main()` runs (debug builds only). This guide covers how to configure the output path.

---

## Recommended: `[package.metadata.rorpc]`

Add the path to your `Cargo.toml`. The macro reads it at compile time — no
`build.rs` required.

```toml
[package]
name = "my-server"
version = "0.1.0"
edition = "2021"

[package.metadata.rorpc]
client_path = "../client/src/rpc/bindings.ts"
```

Then annotate `main()` with no arguments:

```rust
#[rorpc::contract]
#[tokio::main]
async fn main() {
    let app = rorpc::router!(state);
    axum::serve(listener, app).await.unwrap();
}
```

**How it works at compile time:**

1. `#[rorpc::contract]` is expanded by the proc-macro
2. The macro reads `$CARGO_MANIFEST_DIR/Cargo.toml`
3. Extracts `package.metadata.rorpc.client_path`
4. Resolves it to an absolute path (relative to `CARGO_MANIFEST_DIR`)
5. Bakes the absolute path in as a string literal

**Why this is the best default:**

- ✅ No `build.rs` needed
- ✅ Path lives next to other package metadata
- ✅ Version controlled, visible to tooling
- ✅ Works on all platforms (path normalized at compile time)

---

## Alternative: Explicit path in attribute

If you prefer the path in code rather than configuration.

### String literal

```rust
#[rorpc::contract("../client/src/rpc/bindings.ts")]
#[tokio::main]
async fn main() { }
```

> **Note:** Must be an absolute path or a path without `..` components, since
> `generate_contract().output()` requires absolute paths. Use `concat!` with
> `CARGO_MANIFEST_DIR` to construct one.

### `concat!` expression (recommended for explicit paths)

```rust
#[rorpc::contract(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../client/src/rpc/bindings.ts"
))]
#[tokio::main]
async fn main() { }
```

### Constant

```rust
const CLIENT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../client/src/rpc/bindings.ts"
);

#[rorpc::contract(CLIENT_PATH)]
#[tokio::main]
async fn main() { }
```

---

## Fallback: `env!("RORPC_CLIENT_PATH")`

If no argument is given and `[package.metadata.rorpc]` is absent, the macro
falls back to `env!("RORPC_CLIENT_PATH")`. Set it via `build.rs`:

```rust
fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    println!(
        "cargo:rustc-env=RORPC_CLIENT_PATH={}/{}",
        manifest_dir,
        "../client/src/rpc/bindings.ts"
    );
}
```

Or `.cargo/config.toml` (sets runtime env, not compile-time — requires `build.rs` bridge to work with `env!()`).

---

## Option Comparison

| Method | Setup | `build.rs` needed | Path in code |
|--------|-------|:-----------------:|:------------:|
| `[package.metadata.rorpc]` ← recommended | 2 lines in `Cargo.toml` | No | No |
| `#[contract(concat!(...))]` | Inline | No | Yes |
| `#[contract(CLIENT_PATH)]` | `const` + inline | No | Yes |
| `env!("RORPC_CLIENT_PATH")` fallback | `build.rs` | Yes | No |

---

## Troubleshooting

### `contract generation failed: relative output path must not contain '..' components`

The path passed to `output()` must be absolute. When using the metadata approach
(`[package.metadata.rorpc]`), the macro resolves the path to absolute automatically.
When using an explicit string literal, use `concat!(env!("CARGO_MANIFEST_DIR"), "/...")`.

### `environment variable RORPC_CLIENT_PATH not defined`

No argument was given, `[package.metadata.rorpc]` is absent, and
`RORPC_CLIENT_PATH` was not set. Add `client_path` to `[package.metadata.rorpc]`
or pass the path explicitly to the attribute.

### Path resolves to the wrong location

`client_path` is resolved relative to `CARGO_MANIFEST_DIR` (the directory
containing `Cargo.toml`). Example for a workspace layout:

```
workspace/
  server/Cargo.toml      ← CARGO_MANIFEST_DIR
  client/src/rpc/
```

```toml
# server/Cargo.toml
[package.metadata.rorpc]
client_path = "../client/src/rpc/bindings.ts"
```
