//! Zod schema registration — collects TypeScript schema strings at link time.
//!
//! The `#[orpc]` macro emits `inventory::submit! { SchemaRegistration { ... } }`
//! for each input/output type it sees. `generate_contract()` then collects all
//! registered schemas and embeds them in the generated TypeScript file.
//!
//! ## How it works without manual `#[derive(ZodTs)]`
//!
//! Users annotate their types with `#[derive(ZodTs)]` from `zod-rs-ts`.
//! The `#[orpc]` macro emits a `SchemaRegistration` that captures:
//! - The Rust type name (for deduplication)
//! - A factory `fn() -> String` that calls `T::zod_ts()` at runtime
//!
//! This means the schema string is generated lazily at `generate_contract()` time,
//! not at macro expansion time.

// ---------------------------------------------------------------------------
// Structured schema definition — the data-oriented replacement for zod_ts()
// ---------------------------------------------------------------------------

/// A single field in a struct schema.
///
/// Exactly one of `zod_expr` or `type_ref` is non-empty:
/// - `zod_expr` — complete primitive expression, e.g. `"z.string().min(1)"`.
/// - `type_ref`  — bare Rust type name of a custom type, e.g. `"Session"`.
///   The resolution pass maps this to the correct TS schema name.
#[derive(Debug, Clone, Copy)]
pub struct FieldDef {
    /// TypeScript key (serde-renamed), e.g. `"sessionId"`.
    pub ts_name: &'static str,
    /// Complete Zod primitive expression, or `""` when `type_ref` is set.
    pub zod_expr: &'static str,
    /// Bare Rust type name of a custom dependency, or `""` when `zod_expr` is set.
    pub type_ref: &'static str,
    pub optional: bool,
}

/// Serde enum representation strategy.
///
/// Corresponds to `#[serde(tag = "...", content = "...")]` and `#[serde(untagged)]`
/// container attributes.
#[derive(Debug, Clone, Copy)]
pub enum EnumRepr {
    /// Default: `{ "Variant": { fields } }` — no serde annotation
    External,
    /// `#[serde(tag = "t")]` — tag merged into fields: `{ "t": "Variant", ...fields }`
    Internal { tag: &'static str },
    /// `#[serde(tag = "t", content = "c")]` — tag + content: `{ "t": "Variant", "c": { fields } }`
    Adjacent {
        tag: &'static str,
        content: &'static str,
    },
    /// `#[serde(untagged)]` — no discriminator: `{ fields }`
    Untagged,
}

/// A single variant in an enum schema.
#[derive(Debug, Clone, Copy)]
pub struct VariantDef {
    /// Serialized name (serde-renamed), e.g. `"connected"`.
    pub serialized_name: &'static str,
    pub kind: VariantKind,
}

#[derive(Debug, Clone, Copy)]
pub enum VariantKind {
    /// `z.literal("name")`
    Unit,
    /// Newtype variant with a primitive Zod expression.
    NewtypeZod { zod_expr: &'static str },
    /// Newtype variant referencing a custom type.
    NewtypeRef { type_ref: &'static str },
    /// Struct variant: inline object with named fields.
    Struct { fields: &'static [FieldDef] },
}

/// Structured description of a schema's shape.
///
/// Replaces the `zod_ts: fn() -> String` closure. The runtime resolution pass
/// converts this into a `ResolvedSchema` (in `codegen/ir.rs`) before emission.
#[derive(Debug, Clone, Copy)]
pub enum SchemaDef {
    Object {
        fields: &'static [FieldDef],
    },
    Enum {
        repr: EnumRepr,
        variants: &'static [VariantDef],
    },
    /// Used by handler-macro fallback registrations that have no structural
    /// information (the user hasn't added `#[derive(ZodTs)]` yet).
    Unknown,
}

// ---------------------------------------------------------------------------
// SchemaRegistration
// ---------------------------------------------------------------------------

/// A registered Zod schema for a single Rust type.
///
/// Registered globally by the `#[orpc]` macro via `inventory::submit!`.
pub struct SchemaRegistration {
    /// Rust type name for deduplication (e.g. `"Planet"`)
    pub type_name: &'static str,
    /// Full module path (e.g. `"crate::entities::Session"` or `"crate::types::Session"`)
    pub module_path: &'static str,
    /// Structured schema definition emitted by `#[derive(ZodTs)]`.
    /// Fallback handler registrations use `SchemaDef::Unknown`.
    pub schema_def: SchemaDef,
    /// Factory that returns names of nested custom types this type depends on.
    /// Kept for topological sort; values are bare Rust type names.
    pub dependent_types: fn() -> Vec<&'static str>,
}

inventory::collect!(SchemaRegistration);
