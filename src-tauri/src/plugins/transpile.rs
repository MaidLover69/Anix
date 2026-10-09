//! TypeScript payloads are turned into JavaScript before they are run, as Seanime does.

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer};
use std::path::Path;

const MAX_REPORTED_ERRORS: usize = 3;

fn summarize<T: std::fmt::Display>(stage: &str, problems: impl Iterator<Item = T>) -> String {
    let lines: Vec<String> = problems
        .take(MAX_REPORTED_ERRORS)
        .map(|problem| problem.to_string())
        .collect();
    format!("TypeScript {} failed: {}", stage, lines.join("; "))
}

/// Strips the types from a TypeScript payload and rewrites the constructs that have no
/// JavaScript form (enums, namespaces, parameter properties). The payload is treated as a
/// script, so a top-level `class Provider` stays a global declaration.
pub fn typescript_to_javascript(source: &str) -> Result<String, String> {
    let allocator = Allocator::default();
    let source_type = SourceType::ts().with_script(true);

    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(summarize("parsing", parsed.diagnostics.iter()));
    }
    let mut program = parsed.program;

    let semantic = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .with_enum_eval(true)
        .build(&program);
    let scoping = semantic.semantic.into_scoping();

    let options = TransformOptions::default();
    let transformed = Transformer::new(&allocator, Path::new("plugin.ts"), &options)
        .build_with_scoping(scoping, &mut program);
    if !transformed.diagnostics.is_empty() {
        return Err(summarize("conversion", transformed.diagnostics.iter()));
    }

    Ok(Codegen::new().build(&program).code)
}
