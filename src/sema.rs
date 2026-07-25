//! Semantic analysis: variable binding and scope checking.
//!
//! v0.3 scope is intentionally narrow:
//!
//! 1. Walk every `MATCH` / `OPTIONAL MATCH` clause, collect every
//!    variable name introduced by node patterns and relationship
//!    detail brackets. The collected set is the binding scope of the
//!    query.
//! 2. Walk every expression in `WHERE` / `RETURN` / `ORDER BY` /
//!    `LIMIT` / `SKIP` and check that every `Expr::Variable` resolves
//!    to either a binding or a parameter (which is always external).
//! 3. Also flag references to labels and relationship types against
//!    an optional `Schema` - but only when the user provides one.
//!    Without a schema, the analyzer is silent on labels.
//!
//! Type checking, expression-level type inference, and physical
//! resolution are explicitly out of scope.

use std::collections::HashSet;

use crate::ast::*;

/// User-supplied metadata about the data the query will run against.
/// All methods default to "permissive" (everything is valid) so callers
/// can opt in to validation field-by-field.
pub trait Schema {
    fn has_label(&self, _label: &str) -> bool {
        true
    }
    fn has_rel_type(&self, _rel_type: &str) -> bool {
        true
    }
}

/// Schema impl that approves every label and rel-type. Used as the
/// default so analysis never fails on label/rel-type checks unless a
/// caller opts in to a stricter `Schema`.
pub struct PermissiveSchema;
impl Schema for PermissiveSchema {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemIssue {
    pub severity: SemSeverity,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnalysisReport {
    /// Variable names introduced by MATCH/OPTIONAL MATCH patterns.
    pub bindings: HashSet<String>,
    pub issues: Vec<SemIssue>,
}

impl AnalysisReport {
    pub fn errors(&self) -> impl Iterator<Item = &SemIssue> {
        self.issues
            .iter()
            .filter(|i| matches!(i.severity, SemSeverity::Error))
    }

    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }
}

/// Analyze a parsed query. Uses [`PermissiveSchema`] which approves
/// every label and rel-type. For schema-aware validation, call
/// [`analyze_with`] and pass your own `Schema`.
pub fn analyze(query: &Query) -> AnalysisReport {
    analyze_with(query, &PermissiveSchema)
}

/// Analyze a parsed query against `schema`.
pub fn analyze_with<S: Schema + ?Sized>(query: &Query, schema: &S) -> AnalysisReport {
    let mut report = AnalysisReport::default();

    collect_bindings(query, &mut report.bindings);

    for clause in &query.clauses {
        check_clause(clause, &report.bindings, schema, &mut report.issues);
    }

    report
}

fn collect_bindings(query: &Query, out: &mut HashSet<String>) {
    for clause in &query.clauses {
        match clause {
            Clause::Match(m) => {
                for p in &m.patterns {
                    add_pattern_bindings(p, out);
                }
            }
            Clause::Create(c) => {
                for p in &c.patterns {
                    add_pattern_bindings(p, out);
                }
            }
            Clause::Merge(m) => add_pattern_bindings(&m.pattern, out),
            Clause::Unwind(u) => {
                out.insert(u.alias.clone());
            }
            Clause::With(w) => {
                for item in &w.items {
                    if let Some(alias) = &item.alias {
                        out.insert(alias.clone());
                    }
                }
            }
            _ => {}
        }
    }
}

fn add_pattern_bindings(pattern: &Pattern, out: &mut HashSet<String>) {
    add_node_binding(&pattern.anchor, out);
    for chain in &pattern.chain {
        add_rel_binding(&chain.rel, out);
        add_node_binding(&chain.node, out);
    }
}

fn add_node_binding(n: &NodePattern, out: &mut HashSet<String>) {
    if let Some(v) = &n.var {
        out.insert(v.clone());
    }
}

fn add_rel_binding(r: &RelPattern, out: &mut HashSet<String>) {
    if let Some(v) = &r.var {
        out.insert(v.clone());
    }
}

fn check_clause<S: Schema + ?Sized>(
    clause: &Clause,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    match clause {
        Clause::Match(m) => {
            for p in &m.patterns {
                check_pattern(p, bindings, schema, issues);
            }
        }
        Clause::Create(c) => {
            for p in &c.patterns {
                check_pattern(p, bindings, schema, issues);
            }
        }
        Clause::Merge(m) => {
            check_pattern(&m.pattern, bindings, schema, issues);
            for action in &m.actions {
                check_set_items(&action.items, bindings, schema, issues);
            }
        }
        Clause::Set(s) => check_set_items(&s.items, bindings, schema, issues),
        Clause::Delete(d) => {
            for expr in &d.expressions {
                check_expr(expr, bindings, issues);
            }
        }
        Clause::Unwind(u) => check_expr(&u.expr, bindings, issues),
        Clause::Where(e) => check_expr(e, bindings, issues),
        Clause::Return(r) | Clause::With(r) => {
            for item in &r.items {
                check_expr(&item.expr, bindings, issues);
            }
        }
        Clause::OrderBy(items) => {
            for item in items {
                check_expr(&item.expr, bindings, issues);
            }
        }
        Clause::Limit(e) | Clause::Skip(e) => check_expr(e, bindings, issues),
    }
}

fn check_pattern<S: Schema + ?Sized>(
    pattern: &Pattern,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    check_node_pattern(&pattern.anchor, bindings, schema, issues);
    for chain in &pattern.chain {
        check_rel_pattern(&chain.rel, bindings, schema, issues);
        check_node_pattern(&chain.node, bindings, schema, issues);
    }
}

fn check_set_items<S: Schema + ?Sized>(
    items: &[SetItem],
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    for item in items {
        match item {
            SetItem::Property { property, value } => {
                check_expr(property, bindings, issues);
                check_expr(value, bindings, issues);
            }
            SetItem::AllProperties { variable, value }
            | SetItem::MergeProperties { variable, value } => {
                check_expr(&Expr::Variable(variable.clone()), bindings, issues);
                check_expr(value, bindings, issues);
            }
            SetItem::Labels { variable, labels } => {
                check_expr(&Expr::Variable(variable.clone()), bindings, issues);
                for label in labels {
                    if !schema.has_label(label) {
                        issues.push(SemIssue {
                            severity: SemSeverity::Error,
                            code: "unknown-label",
                            message: format!("unknown label `{label}`"),
                        });
                    }
                }
            }
        }
    }
}

fn check_node_pattern<S: Schema + ?Sized>(
    n: &NodePattern,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    for label in &n.labels {
        if !schema.has_label(label) {
            issues.push(SemIssue {
                severity: SemSeverity::Error,
                code: "unknown-label",
                message: format!("unknown label `{label}`"),
            });
        }
    }
    for (_, value) in &n.properties {
        check_expr(value, bindings, issues);
    }
}

fn check_rel_pattern<S: Schema + ?Sized>(
    r: &RelPattern,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    for ty in &r.types {
        if !schema.has_rel_type(ty) {
            issues.push(SemIssue {
                severity: SemSeverity::Error,
                code: "unknown-rel-type",
                message: format!("unknown relationship type `{ty}`"),
            });
        }
    }
    for (_, value) in &r.properties {
        check_expr(value, bindings, issues);
    }
}

fn check_expr(expr: &Expr, bindings: &HashSet<String>, issues: &mut Vec<SemIssue>) {
    match expr {
        Expr::Variable(name) => {
            if !bindings.contains(name) {
                issues.push(SemIssue {
                    severity: SemSeverity::Error,
                    code: "unbound-variable",
                    message: format!(
                        "unbound variable `{name}` (introduce it in a MATCH pattern, or use $name for a parameter)"
                    ),
                });
            }
        }
        Expr::Property { base, .. } => check_expr(base, bindings, issues),
        Expr::Binary { lhs, rhs, .. } => {
            check_expr(lhs, bindings, issues);
            check_expr(rhs, bindings, issues);
        }
        Expr::Unary { operand, .. } => check_expr(operand, bindings, issues),
        Expr::List(items) => {
            for item in items {
                check_expr(item, bindings, issues);
            }
        }
        Expr::Map(entries) => {
            for (_k, v) in entries {
                check_expr(v, bindings, issues);
            }
        }
        Expr::Literal(_) | Expr::Param(_) => {}
    }
}
