//! `cypher-rs` - openCypher front-end in Rust.
//!
//! Pre-v0. Parses a subset of openCypher including read clauses (`MATCH`,
//! `LOAD CSV`, `WHERE`, `RETURN`, `WITH`, ordering and pagination) and update
//! clauses (`CREATE`, `CREATE UNIQUE`, `MERGE`, `SET`, `REMOVE`, `DELETE`,
//! `UNWIND`, `FOREACH`), legacy `START` lookups,
//! the `EXPLAIN`, `PROFILE`, `CYPHER`, and `USING PERIODIC COMMIT` query
//! options, index and constraint schema commands, plus common expressions.
//! Semantic analysis includes conservative expression type inference and
//! statically-known operand and clause-context validation, with optional
//! schema metadata for variables, properties, parameters, and functions.
//!
//! ```
//! use cypher_rs::parse;
//! let q = parse("MATCH (u:User) WHERE u.id = $uid RETURN u.name").unwrap();
//! assert_eq!(q.clauses.len(), 3);
//! ```
//!
//! Roadmap and scope: see the project README.

pub mod ast;
pub mod cost;
pub mod error;
pub mod optimize;
mod parser;
pub mod plan;
pub mod prune;
pub mod sema;

#[cfg(feature = "mcp")]
pub mod mcp;

pub use ast::*;
pub use cost::{estimate, estimate_cost, CardinalityCostModel, CostModel, Estimate};
pub use error::ParseError;
pub use optimize::optimize;
pub use parser::parse;
pub use plan::{plan, Plan, PlanError, ProjectExpr, SortKey};
pub use prune::{output_columns, required_input_columns};
pub use sema::{
    analyze, analyze_with, infer_expression_type, infer_expression_type_with, AnalysisReport,
    CypherType, FunctionSignature, PermissiveSchema, Schema, SemIssue, SemSeverity,
};
