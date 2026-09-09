//! Intermediate representation for resolved Zod schemas.
//!
//! After the registry resolves all name collisions and cross-references,
//! each schema is represented as a `ResolvedSchema` before TypeScript is
//! emitted. This keeps emission a single, trivial pass over structured data
//! rather than post-hoc string rewriting.

/// Resolved enum representation strategy.
///
/// Mirrors `EnumRepr` from `schema_registry.rs` but uses owned `String`s
/// since resolved IR is heap-allocated.
#[derive(Debug, Clone)]
pub enum ResolvedEnumRepr {
    External,
    Internal { tag: String },
    Adjacent { tag: String, content: String },
    Untagged,
}

/// A fully resolved schema ready for one-pass TypeScript emission.
#[derive(Debug, Clone)]
pub struct ResolvedSchema {
    /// Disambiguated TypeScript constant name, e.g. `"TypesSessionSchema"`.
    pub ts_schema_name: String,
    /// Type alias name (schema name without the `Schema` suffix),
    /// e.g. `"TypesSession"`.
    pub ts_type_name: String,
    /// Full Rust module path of the originating type,
    /// e.g. `"issue1_duplicate_schemas::types::Session"`.
    /// Empty string for fallback (Unknown) registrations.
    pub module_path: &'static str,
    /// Shape of the schema.
    pub def: ResolvedDef,
}

/// The shape of a resolved schema.
#[derive(Debug, Clone)]
pub enum ResolvedDef {
    Object {
        fields: Vec<ResolvedField>,
    },
    Enum {
        repr: ResolvedEnumRepr,
        variants: Vec<ResolvedVariant>,
    },
}

/// A single field inside a resolved `z.object({...})`.
#[derive(Debug, Clone)]
pub struct ResolvedField {
    /// TypeScript key, already serde-renamed if applicable, e.g. `"sessionId"`.
    pub ts_key: String,
    /// Complete Zod expression for this field, e.g.:
    /// - `"z.string().min(1)"` for a primitive
    /// - `"TypesSessionSchema.optional()"` for a resolved cross-reference
    pub zod_expr: String,
}

/// A single variant inside a resolved `z.union([...])`.
#[derive(Debug, Clone)]
pub struct ResolvedVariant {
    /// Serialized name, e.g. `"connected"`.
    pub serialized_name: String,
    pub kind: ResolvedVariantKind,
}

#[derive(Debug, Clone)]
pub enum ResolvedVariantKind {
    /// `z.literal("connected")`
    Unit,
    /// `z.object({ tag: <expr> })`
    Newtype { zod_expr: String },
    /// `z.object({ tag: z.object({ field: zod_expr, ... }) })`
    Struct { fields: Vec<ResolvedField> },
}
