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
    parse::{Parse, ParseStream},
    Expr, ItemFn,
};

/// Parsed arguments for `#[contract(...)]` attribute.
///
/// Supports:
/// - `#[contract]` — reads `[package.metadata.rorpc] client_path` from `Cargo.toml`,
///                   falls back to `env!("RORPC_CLIENT_PATH")`
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

/// Try to read `[package.metadata.rorpc] client_path` from the crate's `Cargo.toml`.
///
/// Called at macro expansion time. Returns `Some(absolute_path)` if the key is
/// present, `None` otherwise. The relative path is resolved against
/// `CARGO_MANIFEST_DIR` so `output()` always receives an absolute path.
fn read_metadata_client_path() -> Option<String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let cargo_toml_path = std::path::Path::new(&manifest_dir).join("Cargo.toml");
    let content = std::fs::read_to_string(cargo_toml_path).ok()?;
    let manifest: toml::Value = toml::from_str(&content).ok()?;

    let client_path = manifest
        .get("package")?
        .get("metadata")?
        .get("rorpc")?
        .get("client_path")?
        .as_str()?;

    // Resolve relative to CARGO_MANIFEST_DIR — output() requires an absolute path
    let absolute = std::path::Path::new(&manifest_dir)
        .join(client_path)
        .to_string_lossy()
        .into_owned();

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

    #[test]
    fn parse_empty_args() {
        let args: ContractArgs = syn::parse2(quote! {}).expect("parse failed");
        assert!(args.path_expr.is_none());
    }

    #[test]
    fn parse_string_literal() {
        let args: ContractArgs = syn::parse2(quote! { "../client/bindings.ts" })
            .expect("parse failed");
        assert!(args.path_expr.is_some());
    }

    #[test]
    fn parse_env_macro() {
        let args: ContractArgs = syn::parse2(quote! { env!("RORPC_CLIENT_PATH") })
            .expect("parse failed");
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

    #[test]
    fn expand_with_string_literal() {
        let func: ItemFn = syn::parse2(quote! {
            fn main() { println!("Hello"); }
        })
        .expect("parse failed");

        let args: ContractArgs = syn::parse2(quote! { "../client/bindings.ts" })
            .expect("parse failed");
        let expanded = expand_contract(args, func);
        let s = expanded.to_string();

        assert!(s.contains("\"../client/bindings.ts\""));
        assert!(s.contains("rorpc :: generate_contract"));
        assert!(s.contains("#[cfg(debug_assertions)]"));
    }

    #[test]
    fn expand_preserves_attributes() {
        let func: ItemFn = syn::parse2(quote! {
            #[tokio::main]
            async fn main() { println!("Hello"); }
        })
        .expect("parse failed");

        let args: ContractArgs = syn::parse2(quote! { "../client/bindings.ts" })
            .expect("parse failed");
        let expanded = expand_contract(args, func);
        let s = expanded.to_string();

        assert!(s.contains("#[tokio :: main]"));
        assert!(s.contains("async fn main"));
    }
}
