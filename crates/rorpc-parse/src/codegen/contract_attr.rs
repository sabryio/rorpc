//! Code generation for the `#[contract]` attribute macro.
//!
//! Wraps `fn main()` to automatically call `rorpc::generate_contract().output(path)`
//! in debug builds. Supports compile-time path expressions including `env!()`,
//! `concat!()`, string literals, and constants.
//!
//! Resolution order when no argument is provided:
//! 1. `[package.metadata.rorpc] client_path` in `Cargo.toml` (read at macro expansion time)
//! 2. `env!("RORPC_CLIENT_PATH")` fallback

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Expr, ItemFn,
    parse::{Parse, ParseStream},
};

/// Parsed arguments for `#[contract(...)]` attribute.
///
/// Supports:
/// - `#[contract]` — reads `[package.metadata.rorpc] client_path` from `Cargo.toml`,
///   falls back to `env!("RORPC_CLIENT_PATH")`
/// - `#[contract("../client/bindings.ts")]` — string literal
/// - `#[contract(env!("RORPC_CLIENT_PATH"))]` — environment variable
/// - `#[contract(concat!(...))]` — concatenation expression
/// - `#[contract(CLIENT_PATH)]` — constant
pub struct ContractArgs {
    /// The compile-time expression for the output path.
    /// If `None`, resolved from `Cargo.toml` metadata or env var.
    pub path_expr: Option<Expr>,
}

impl Parse for ContractArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(ContractArgs { path_expr: None });
        }
        let expr: Expr = input.parse()?;
        Ok(ContractArgs {
            path_expr: Some(expr),
        })
    }
}

/// Normalize a path by resolving `.` and `..` components without requiring the
/// path to exist on disk. Unlike [`std::fs::canonicalize`], this works for
/// paths that haven't been created yet (e.g., a TypeScript output file that
/// will be generated for the first time).
///
/// Walks each component and maintains a stack:
/// - `..` pops the last element (won't go above a prefix/root component)
/// - `.` is skipped
/// - Everything else is pushed
fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;

    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                // Windows drive prefix (e.g. "D:") — always first, reset stack
                stack.clear();
                stack.push(component.as_os_str().to_owned());
            }
            Component::RootDir => {
                // Root separator — keep alongside prefix, don't wipe it
                stack.push(component.as_os_str().to_owned());
            }
            Component::CurDir => {
                // `.` — skip
            }
            Component::ParentDir => {
                // `..` — pop only if the top of the stack is a Normal segment.
                // Never pop a Prefix ("D:") or RootDir ("\") entry.
                let last_is_normal = stack
                    .last()
                    .map(|s| {
                        let p = std::path::Path::new(s);
                        matches!(p.components().next(), Some(Component::Normal(_)))
                    })
                    .unwrap_or(false);
                if last_is_normal {
                    stack.pop();
                }
            }
            Component::Normal(name) => {
                stack.push(name.to_owned());
            }
        }
    }

    stack.iter().collect()
}

/// Try to read `[package.metadata.rorpc] client_path` from the crate's `Cargo.toml`.
///
/// Called at macro expansion time. Returns `Some(absolute_path)` if the key is
/// present, `None` otherwise. The relative path is resolved against
/// `CARGO_MANIFEST_DIR` so `output()` always receives an absolute path.
///
/// Uses [`normalize_path`] instead of `canonicalize` so that `..` components
/// are resolved purely lexically — no filesystem access required, meaning paths
/// that point outside the Rust workspace (e.g.
/// `"../../../../frontend/src/rpc/bindings.ts"`) work even before the target
/// file exists.
fn read_metadata_client_path() -> Option<String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let cargo_toml_path = std::path::Path::new(&manifest_dir).join("Cargo.toml");
    let content = std::fs::read_to_string(cargo_toml_path).ok()?;
    let manifest: toml::Value = toml::from_str(&content).ok()?;

    let client_path = manifest
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("rorpc"))
        .and_then(|r| r.get("client_path"))
        .and_then(|v| v.as_str())?;

    // Join onto CARGO_MANIFEST_DIR, then normalize to resolve any .. / . components
    // without requiring the target path to exist on disk.
    let joined = std::path::Path::new(&manifest_dir).join(client_path);
    let absolute = normalize_path(&joined).to_string_lossy().into_owned();

    Some(absolute)
}

/// Expand `#[contract(...)] fn main() { ... }` into:
///
/// ```ignore
/// fn main() {
///     #[cfg(debug_assertions)]
///     {
///         rorpc::generate_contract()
///             .output(path)
///             .expect("contract generation failed");
///     }
///     // original body
/// }
/// ```
pub fn expand_contract(args: ContractArgs, func: ItemFn) -> TokenStream {
    let ItemFn {
        attrs,
        vis,
        sig,
        block,
        ..
    } = func;

    let original_body = &block.stmts;

    // Resolution order:
    // 1. Explicit argument passed to the macro
    // 2. [package.metadata.rorpc] client_path in Cargo.toml (read at compile time)
    // 3. env!("RORPC_CLIENT_PATH") fallback
    let path_tokens: TokenStream = if let Some(expr) = args.path_expr {
        quote! { #expr }
    } else if let Some(path) = read_metadata_client_path() {
        // Bake the resolved absolute path in as a string literal
        quote! { #path }
    } else {
        quote! { env!("RORPC_CLIENT_PATH") }
    };

    quote! {
        #(#attrs)*
        #vis #sig {
            #[cfg(debug_assertions)]
            {
                ::rorpc::generate_contract()
                    .output(#path_tokens)
                    .expect("contract generation failed");
            }

            #(#original_body)*
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    // ── ContractArgs parsing ────────────────────────────────────────────────

    #[test]
    fn parse_empty_args() {
        let args: ContractArgs = syn::parse2(quote! {}).expect("parse failed");
        assert!(args.path_expr.is_none());
    }

    #[test]
    fn parse_string_literal() {
        let args: ContractArgs =
            syn::parse2(quote! { "../client/bindings.ts" }).expect("parse failed");
        assert!(args.path_expr.is_some());
    }

    #[test]
    fn parse_env_macro() {
        let args: ContractArgs =
            syn::parse2(quote! { env!("RORPC_CLIENT_PATH") }).expect("parse failed");
        assert!(args.path_expr.is_some());
    }

    #[test]
    fn parse_concat_macro() {
        let args: ContractArgs = syn::parse2(quote! {
            concat!(env!("CARGO_MANIFEST_DIR"), "/../client/src/rpc/bindings.ts")
        })
        .expect("parse failed");
        assert!(args.path_expr.is_some());
    }

    #[test]
    fn parse_constant() {
        let args: ContractArgs = syn::parse2(quote! { CLIENT_PATH }).expect("parse failed");
        assert!(args.path_expr.is_some());
    }

    // ── expand_contract code generation ────────────────────────────────────

    #[test]
    fn expand_with_string_literal() {
        let func: ItemFn = syn::parse2(quote! {
            fn main() { println!("Hello"); }
        })
        .expect("parse failed");

        let args: ContractArgs =
            syn::parse2(quote! { "../client/bindings.ts" }).expect("parse failed");
        let expanded = expand_contract(args, func);
        let s = expanded.to_string();

        assert!(s.contains("\"../client/bindings.ts\""));
        assert!(s.contains("rorpc :: generate_contract"));
        assert!(s.contains("# [cfg (debug_assertions)]") || s.contains("#[cfg(debug_assertions)]"));
    }

    #[test]
    fn expand_preserves_attributes() {
        let func: ItemFn = syn::parse2(quote! {
            #[tokio::main]
            async fn main() { println!("Hello"); }
        })
        .expect("parse failed");

        let args: ContractArgs =
            syn::parse2(quote! { "../client/bindings.ts" }).expect("parse failed");
        let expanded = expand_contract(args, func);
        let s = expanded.to_string();

        assert!(s.contains("# [tokio :: main]") || s.contains("#[tokio::main]"));
        assert!(s.contains("async fn main"));
    }

    // ── normalize_path — Unix ───────────────────────────────────────────────

    #[test]
    fn normalize_simple_parent_traversal() {
        // ../.. from /repo/server/crate → /repo
        let p = std::path::Path::new("/repo/server/crate").join("../../out.ts");
        assert_eq!(normalize_path(&p), std::path::Path::new("/repo/out.ts"));
    }

    #[test]
    fn normalize_sibling_dir() {
        // CARGO_MANIFEST_DIR=/repo/server, client_path="../client/src/bindings.ts"
        let p = std::path::Path::new("/repo/server").join("../client/src/bindings.ts");
        assert_eq!(
            normalize_path(&p),
            std::path::Path::new("/repo/client/src/bindings.ts")
        );
    }

    #[test]
    fn normalize_deep_traversal_stops_at_root() {
        // More `..` than path segments — must not go above /
        let p = std::path::Path::new("/a/b").join("../../../../out.ts");
        assert_eq!(normalize_path(&p), std::path::Path::new("/out.ts"));
    }

    #[test]
    fn normalize_curdirs_are_skipped() {
        let p = std::path::Path::new("/repo/./server/./crate").join("./out.ts");
        assert_eq!(
            normalize_path(&p),
            std::path::Path::new("/repo/server/crate/out.ts")
        );
    }

    #[test]
    fn normalize_already_clean_path_unchanged() {
        let p = std::path::Path::new("/repo/client/src/bindings.ts");
        assert_eq!(normalize_path(p), p);
    }

    // ── normalize_path — Windows ────────────────────────────────────────────

    #[cfg(windows)]
    #[test]
    fn normalize_windows_preserves_drive_letter_shallow() {
        // The original bug: drive letter was wiped when RootDir cleared the stack.
        // CARGO_MANIFEST_DIR = D:\programming\Rust\rust-orpc\examples\axum-react\better-auth-integration
        // client_path = "../client/src/rpc/bindings.ts"
        let base = std::path::Path::new(
            r"D:\programming\Rust\rust-orpc\examples\axum-react\better-auth-integration",
        );
        let p = base.join("../client/src/rpc/bindings.ts");
        assert_eq!(
            normalize_path(&p),
            std::path::Path::new(
                r"D:\programming\Rust\rust-orpc\examples\axum-react\client\src\rpc\bindings.ts"
            ),
        );
    }

    #[cfg(windows)]
    #[test]
    fn normalize_windows_deep_traversal_to_near_root() {
        // 5 `..` from a 5-segment path lands just inside the drive root.
        // better-auth-integration → axum-react → examples → rust-orpc → Rust → programming
        let base = std::path::Path::new(
            r"D:\programming\Rust\rust-orpc\examples\axum-react\better-auth-integration",
        );
        let p = base.join("../../../../../out.ts");
        assert_eq!(
            normalize_path(&p),
            std::path::Path::new(r"D:\programming\out.ts"),
        );
    }

    #[cfg(windows)]
    #[test]
    fn normalize_windows_excessive_traversal_stops_at_root() {
        // More `..` than segments — must not eat the drive letter or root separator.
        let base = std::path::Path::new(r"D:\a\b");
        let p = base.join("../../../../../out.ts");
        assert_eq!(normalize_path(&p), std::path::Path::new(r"D:\out.ts"));
    }

    #[cfg(windows)]
    #[test]
    fn normalize_windows_no_traversal_unchanged() {
        let p = std::path::Path::new(r"D:\programming\Rust\client\src\bindings.ts");
        assert_eq!(normalize_path(p), p);
    }
}
