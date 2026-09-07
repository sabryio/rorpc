//! Code generation for the `#[rorpc::namespace("/prefix")]` attribute macro.
//!
//! Registers a namespace prefix for a module. All handlers within the module
//! will have their paths prefixed with this namespace at runtime (during router
//! construction and contract generation).

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    Item, LitStr,
};

use crate::errors::Result;

/// Parsed arguments for the `#[rorpc::namespace("/prefix")]` attribute.
#[derive(Debug)]
pub struct NamespaceArgs {
    pub prefix: String,
}

impl Parse for NamespaceArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit: LitStr = input.parse()?;
        let prefix = lit.value();

        // Validate prefix
        if !prefix.starts_with('/') {
            return Err(syn::Error::new(
                lit.span(),
                "namespace prefix must start with '/'",
            ));
        }

        if prefix.contains("..") {
            return Err(syn::Error::new(
                lit.span(),
                "namespace prefix cannot contain '..' path traversal",
            ));
        }

        if prefix.ends_with('/') && prefix.len() > 1 {
            return Err(syn::Error::new(
                lit.span(),
                "namespace prefix should not end with '/' (except for root)",
            ));
        }

        Ok(NamespaceArgs { prefix })
    }
}

/// Expand the `#[rorpc::namespace("/prefix")]` attribute.
///
/// Returns the original item unchanged plus an `inventory::submit!` registration
/// for `NamespaceMetadata`.
pub fn expand_namespace(args: NamespaceArgs, item: Item) -> Result<TokenStream> {
    let prefix = &args.prefix;

    Ok(quote! {
        #item

        ::rorpc::inventory::submit! {
            ::rorpc::NamespaceMetadata {
                module_path: ::std::module_path!(),
                prefix: #prefix,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_prefix() {
        let args: NamespaceArgs = syn::parse_str("\"/planet\"").unwrap();
        assert_eq!(args.prefix, "/planet");
    }

    #[test]
    fn parse_root_prefix() {
        let args: NamespaceArgs = syn::parse_str("\"/\"").unwrap();
        assert_eq!(args.prefix, "/");
    }

    #[test]
    fn reject_missing_leading_slash() {
        let result: syn::Result<NamespaceArgs> = syn::parse_str("\"planet\"");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must start with '/'"));
    }

    #[test]
    fn reject_path_traversal() {
        let result: syn::Result<NamespaceArgs> = syn::parse_str("\"/planet/../admin\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(".."));
    }

    #[test]
    fn reject_trailing_slash() {
        let result: syn::Result<NamespaceArgs> = syn::parse_str("\"/planet/\"");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("should not end with '/'"));
    }
}
