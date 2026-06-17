//! Compact declaration of command output schemas.
//!
//! cli-engine surfaces a command's registered output schema in two places: the
//! `--help` text gets an "Output fields" section listing every field, and
//! `--schema` dumps the field metadata as JSON. Commands opt in by calling
//! [`CommandSpec::with_output_schema::<T>()`](cli_engine::CommandSpec::with_output_schema)
//! with a type implementing [`cli_engine::OutputSchema`].
//!
//! Hand-writing those impls for every command is noisy, so [`output_schema!`]
//! declares a zero-sized marker type and its `OutputSchema` impl from a terse
//! field list. The `field_type` strings are display-only (shown verbatim in
//! help), so they describe the JSON shape — `string`, `bool`, `number`,
//! `object`, `[]string`, `[]object`.
//!
//! These describe the shape each command's handler emits; keep them in sync with
//! the handler's `json!` output. They are informational (help/`--schema` only)
//! and never affect the actual command output.

/// Declares a marker type and its [`cli_engine::OutputSchema`] impl.
///
/// ```ignore
/// output_schema!(EnvSummary {
///     "name": "string";
///     "active": "bool";
///     "apiUrl": "string";
/// });
/// // ...later:
/// CommandSpec::new("list", "…").with_output_schema::<EnvSummary>()
/// ```
///
/// Append `, optional` to a field to mark it optional (renders `(optional)` in
/// help):
///
/// ```ignore
/// output_schema!(AppSummary { "id": "string"; "description": "string", optional; });
/// ```
macro_rules! output_schema {
    ($name:ident { $($field:literal : $ty:literal $(, $opt:ident)? );* $(;)? }) => {
        pub(crate) struct $name;

        impl cli_engine::OutputSchema for $name {
            fn fields() -> &'static [cli_engine::OutputField] {
                &[ $(
                    cli_engine::OutputField {
                        name: $field,
                        field_type: $ty,
                        optional: output_schema!(@opt $($opt)?),
                    }
                ),* ]
            }
        }
    };
    (@opt optional) => { true };
    (@opt) => { false };
}

pub(crate) use output_schema;
