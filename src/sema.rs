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
//! Expression types are inferred conservatively: values whose type depends on
//! runtime parameters or schema metadata remain [`CypherType::Any`].

use std::collections::{HashMap, HashSet};

use crate::ast::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CypherType {
    Any,
    Null,
    Boolean,
    Integer,
    Float,
    String,
    List(Box<CypherType>),
    Map,
    Node,
    Relationship,
    Path,
    Point,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSignature {
    pub arguments: Vec<CypherType>,
    pub variadic: bool,
    pub result: CypherType,
}

struct ScopedSchema<'a, S: ?Sized> {
    base: &'a S,
    variables: &'a HashMap<String, CypherType>,
}

impl<S: Schema + ?Sized> Schema for ScopedSchema<'_, S> {
    fn has_label(&self, label: &str) -> bool {
        self.base.has_label(label)
    }

    fn has_rel_type(&self, rel_type: &str) -> bool {
        self.base.has_rel_type(rel_type)
    }

    fn variable_type(&self, variable: &str) -> Option<CypherType> {
        self.variables
            .get(variable)
            .cloned()
            .or_else(|| self.base.variable_type(variable))
    }

    fn property_type(&self, variable: Option<&str>, property: &str) -> Option<CypherType> {
        self.base.property_type(variable, property)
    }

    fn parameter_type(&self, parameter: &str) -> Option<CypherType> {
        self.base.parameter_type(parameter)
    }

    fn function_signature(&self, name: &str) -> Option<FunctionSignature> {
        self.base.function_signature(name)
    }

    fn function_signatures(&self, name: &str) -> Vec<FunctionSignature> {
        self.base.function_signatures(name)
    }
}

impl CypherType {
    fn is_numeric(&self) -> bool {
        matches!(self, Self::Any | Self::Integer | Self::Float)
    }

    fn accepts(&self, actual: &Self) -> bool {
        self == actual
            || matches!(self, Self::Any)
            || matches!(actual, Self::Any | Self::Null)
            || matches!((self, actual), (Self::List(expected), Self::List(actual)) if expected.accepts(actual))
    }
}

impl std::fmt::Display for CypherType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Any => f.write_str("ANY"),
            Self::Null => f.write_str("NULL"),
            Self::Boolean => f.write_str("BOOLEAN"),
            Self::Integer => f.write_str("INTEGER"),
            Self::Float => f.write_str("FLOAT"),
            Self::String => f.write_str("STRING"),
            Self::List(item) => write!(f, "LIST<{item}>"),
            Self::Map => f.write_str("MAP"),
            Self::Node => f.write_str("NODE"),
            Self::Relationship => f.write_str("RELATIONSHIP"),
            Self::Path => f.write_str("PATH"),
            Self::Point => f.write_str("POINT"),
        }
    }
}

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

    fn variable_type(&self, _variable: &str) -> Option<CypherType> {
        None
    }

    fn property_type(&self, _variable: Option<&str>, _property: &str) -> Option<CypherType> {
        None
    }

    fn parameter_type(&self, _parameter: &str) -> Option<CypherType> {
        None
    }

    fn function_signature(&self, _name: &str) -> Option<FunctionSignature> {
        None
    }

    fn function_signatures(&self, name: &str) -> Vec<FunctionSignature> {
        self.function_signature(name).into_iter().collect()
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

    for clauses in query.statements() {
        analyze_statement(clauses, schema, &mut report);
    }

    report
}

fn analyze_statement<S: Schema + ?Sized>(
    clauses: &[Clause],
    schema: &S,
    report: &mut AnalysisReport,
) {
    let mut branch_start = 0;
    for (index, clause) in clauses.iter().enumerate() {
        if matches!(clause, Clause::Union(_)) {
            analyze_branch(&clauses[branch_start..index], schema, report);
            branch_start = index + 1;
        }
    }
    analyze_branch(&clauses[branch_start..], schema, report);
}

fn analyze_branch<S: Schema + ?Sized>(clauses: &[Clause], schema: &S, report: &mut AnalysisReport) {
    let mut bindings = HashSet::new();
    collect_bindings(clauses, &mut bindings);
    let mut types = HashMap::new();
    for clause in clauses {
        {
            let scoped = ScopedSchema {
                base: schema,
                variables: &types,
            };
            check_clause(
                clause,
                &bindings,
                &scoped as &dyn Schema,
                &mut report.issues,
            );
        }
        let snapshot = types.clone();
        let update_schema = ScopedSchema {
            base: schema,
            variables: &snapshot,
        };
        update_type_bindings(clause, &update_schema, &mut types);
    }
    report.bindings.extend(bindings);
}

fn update_type_bindings<S: Schema + ?Sized>(
    clause: &Clause,
    schema: &S,
    types: &mut HashMap<String, CypherType>,
) {
    match clause {
        Clause::Match(m) => {
            for pattern in &m.patterns {
                add_pattern_types(pattern, types);
            }
        }
        Clause::Create(c) => {
            for pattern in &c.patterns {
                add_pattern_types(pattern, types);
            }
        }
        Clause::Merge(m) => add_pattern_types(&m.pattern, types),
        Clause::Start(start) => {
            for point in &start.points {
                types.insert(
                    point.variable.clone(),
                    match point.entity {
                        StartEntity::Node => CypherType::Node,
                        StartEntity::Relationship => CypherType::Relationship,
                    },
                );
            }
        }
        Clause::Unwind(unwind) => {
            let item = list_item_type(&infer_expression_type_with(&unwind.expr, schema));
            types.insert(unwind.alias.clone(), item);
        }
        Clause::LoadCsv(load_csv) => {
            types.insert(load_csv.variable.clone(), CypherType::Map);
        }
        Clause::Call(call) => {
            for item in &call.yields {
                types.insert(item.binding().to_string(), CypherType::Any);
            }
        }
        Clause::With(with) => {
            if !with.include_existing {
                types.clear();
            }
            for item in &with.items {
                if let Some(name) = return_item_name(item) {
                    types.insert(name, infer_expression_type_with(&item.expr, schema));
                }
            }
        }
        _ => {}
    }
}

fn add_pattern_types(pattern: &Pattern, types: &mut HashMap<String, CypherType>) {
    if let Some(path) = &pattern.path_variable {
        types.insert(path.clone(), CypherType::Path);
    }
    if let Some(variable) = &pattern.anchor.var {
        types.insert(variable.clone(), CypherType::Node);
    }
    for chain in &pattern.chain {
        if let Some(variable) = &chain.rel.var {
            types.insert(variable.clone(), CypherType::Relationship);
        }
        if let Some(variable) = &chain.node.var {
            types.insert(variable.clone(), CypherType::Node);
        }
    }
}

fn return_item_name(item: &ReturnItem) -> Option<String> {
    item.alias.clone().or_else(|| match &item.expr {
        Expr::Variable(variable) => Some(variable.clone()),
        _ => None,
    })
}

fn list_item_type(ty: &CypherType) -> CypherType {
    match ty {
        CypherType::List(item) => item.as_ref().clone(),
        _ => CypherType::Any,
    }
}

fn infer_builtin_overload_result<S: Schema + ?Sized>(
    name: &str,
    arguments: &FunctionArguments,
    schema: &S,
) -> CypherType {
    let signatures = builtin_function_signatures(name);
    let FunctionArguments::Expressions(arguments) = arguments else {
        return unify_types(signatures.into_iter().map(|signature| signature.result));
    };
    let actual = arguments
        .iter()
        .map(|argument| infer_expression_type_with(argument, schema))
        .collect::<Vec<_>>();
    let result = unify_types(
        signatures
            .iter()
            .filter(|signature| {
                signature_accepts_arity(signature, actual.len())
                    && actual.iter().enumerate().all(|(index, actual)| {
                        signature_argument(signature, index)
                            .is_some_and(|expected| expected.accepts(actual))
                    })
            })
            .map(|signature| signature.result.clone()),
    );
    if result == CypherType::Null {
        CypherType::Any
    } else {
        result
    }
}

fn collect_bindings(clauses: &[Clause], out: &mut HashSet<String>) {
    for clause in clauses {
        match clause {
            Clause::SchemaCommand(_) => {}
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
            Clause::LoadCsv(load_csv) => {
                out.insert(load_csv.variable.clone());
            }
            Clause::Start(start) => {
                out.extend(start.points.iter().map(|point| point.variable.clone()));
            }
            Clause::Call(call) => {
                out.extend(call.yields.iter().map(|item| item.binding().to_string()));
            }
            Clause::With(w) => {
                for item in &w.items {
                    if let Some(alias) = &item.alias {
                        out.insert(alias.clone());
                    }
                }
            }
            Clause::Union(_) => {}
            _ => {}
        }
    }
}

fn add_pattern_bindings(pattern: &Pattern, out: &mut HashSet<String>) {
    if let Some(variable) = &pattern.path_variable {
        out.insert(variable.clone());
    }
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
        Clause::SchemaCommand(command) => check_schema_command(command, schema, issues),
        Clause::Match(m) => {
            for p in &m.patterns {
                check_pattern(p, bindings, schema, issues);
            }
            for hint in &m.hints {
                check_match_hint(hint, bindings, schema, issues);
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
        Clause::Remove(r) => check_remove_items(&r.items, bindings, schema, issues),
        Clause::Delete(d) => {
            for expr in &d.expressions {
                check_expr(expr, bindings, schema, issues);
            }
        }
        Clause::Unwind(u) => {
            check_expr(&u.expr, bindings, schema, issues);
            expect_type_with(
                &u.expr,
                &CypherType::List(Box::new(CypherType::Any)),
                "UNWIND",
                schema,
                issues,
            );
        }
        Clause::Foreach(foreach) => {
            check_expr(&foreach.expression, bindings, schema, issues);
            expect_type_with(
                &foreach.expression,
                &CypherType::List(Box::new(CypherType::Any)),
                "FOREACH source",
                schema,
                issues,
            );
            let mut local_bindings = bindings.clone();
            local_bindings.insert(foreach.variable.clone());
            collect_bindings(&foreach.clauses, &mut local_bindings);
            let mut local_types = HashMap::new();
            local_types.insert(
                foreach.variable.clone(),
                list_item_type(&infer_expression_type_with(&foreach.expression, schema)),
            );
            for clause in &foreach.clauses {
                {
                    let scoped = ScopedSchema {
                        base: schema,
                        variables: &local_types,
                    };
                    check_clause(clause, &local_bindings, &scoped as &dyn Schema, issues);
                }
                let snapshot = local_types.clone();
                let update_schema = ScopedSchema {
                    base: schema,
                    variables: &snapshot,
                };
                update_type_bindings(clause, &update_schema, &mut local_types);
            }
        }
        Clause::LoadCsv(load_csv) => {
            let mut source_bindings = bindings.clone();
            source_bindings.remove(&load_csv.variable);
            check_expr(&load_csv.url, &source_bindings, schema, issues);
            expect_type_with(
                &load_csv.url,
                &CypherType::String,
                "LOAD CSV URL",
                schema,
                issues,
            );
        }
        Clause::Start(start) => {
            for point in &start.points {
                if let StartLookup::Index { value, .. } = &point.lookup {
                    check_expr(value, bindings, schema, issues);
                }
            }
            if let Some(predicate) = &start.predicate {
                check_expr(predicate, bindings, schema, issues);
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "START WHERE",
                    schema,
                    issues,
                );
            }
        }
        Clause::Call(call) => {
            for argument in &call.arguments {
                check_expr(argument, bindings, schema, issues);
            }
            if let Some(predicate) = &call.predicate {
                check_expr(predicate, bindings, schema, issues);
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "CALL WHERE",
                    schema,
                    issues,
                );
            }
        }
        Clause::Where(e) => {
            check_expr(e, bindings, schema, issues);
            expect_type_with(e, &CypherType::Boolean, "WHERE", schema, issues);
        }
        Clause::Return(r) | Clause::With(r) => {
            for item in &r.items {
                check_expr(&item.expr, bindings, schema, issues);
            }
        }
        Clause::OrderBy(items) => {
            for item in items {
                check_expr(&item.expr, bindings, schema, issues);
            }
        }
        Clause::Limit(e) | Clause::Skip(e) => {
            check_expr(e, bindings, schema, issues);
            expect_type_with(e, &CypherType::Integer, "LIMIT/SKIP", schema, issues);
        }
        Clause::Union(_) => {}
    }
}

fn check_schema_command<S: Schema + ?Sized>(
    command: &SchemaCommand,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    let (variable, expression) = match command {
        SchemaCommand::CreateNodeConstraint {
            variable,
            expression,
            ..
        }
        | SchemaCommand::CreateRelationshipConstraint {
            variable,
            expression,
            ..
        }
        | SchemaCommand::DropNodeConstraint {
            variable,
            expression,
            ..
        }
        | SchemaCommand::DropRelationshipConstraint {
            variable,
            expression,
            ..
        } => (variable, expression),
        SchemaCommand::CreateIndex { .. } | SchemaCommand::DropIndex { .. } => return,
    };
    let bindings = [variable.clone()].into_iter().collect();
    check_expr(expression, &bindings, schema, issues);
}

fn check_match_hint<S: Schema + ?Sized>(
    hint: &MatchHint,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    match hint {
        MatchHint::Index {
            variable, label, ..
        }
        | MatchHint::Scan { variable, label } => {
            check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
            if !schema.has_label(label) {
                issues.push(SemIssue {
                    severity: SemSeverity::Error,
                    code: "unknown-label",
                    message: format!("unknown node label `{label}` in planner hint"),
                });
            }
        }
        MatchHint::Join { variables } => {
            for variable in variables {
                check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
            }
        }
    }
}

fn check_remove_items<S: Schema + ?Sized>(
    items: &[RemoveItem],
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    for item in items {
        match item {
            RemoveItem::Property(property) => check_expr(property, bindings, schema, issues),
            RemoveItem::Labels { variable, labels } => {
                check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
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
                check_expr(property, bindings, schema, issues);
                check_expr(value, bindings, schema, issues);
            }
            SetItem::AllProperties { variable, value }
            | SetItem::MergeProperties { variable, value } => {
                check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
                check_expr(value, bindings, schema, issues);
            }
            SetItem::Labels { variable, labels } => {
                check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
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
        check_expr(value, bindings, schema, issues);
    }
    if let Some(property_map) = &n.property_map {
        check_expr(property_map, bindings, schema, issues);
    }
}

fn check_rel_pattern<S: Schema + ?Sized>(
    r: &RelPattern,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    if let Some(RelationshipRange {
        start: Some(start),
        end: Some(end),
    }) = r.range
    {
        if start > end {
            issues.push(SemIssue {
                severity: SemSeverity::Error,
                code: "invalid-relationship-range",
                message: format!(
                    "relationship length lower bound {start} exceeds upper bound {end}"
                ),
            });
        }
    }
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
        check_expr(value, bindings, schema, issues);
    }
    if let Some(property_map) = &r.property_map {
        check_expr(property_map, bindings, schema, issues);
    }
}

fn check_expr<S: Schema + ?Sized>(
    expr: &Expr,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
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
        Expr::Property { base, .. } => check_expr(base, bindings, schema, issues),
        Expr::LabelPredicate { expression, labels } => {
            check_expr(expression, bindings, schema, issues);
            for label in labels {
                if !schema.has_label(label) {
                    issues.push(SemIssue {
                        severity: SemSeverity::Error,
                        code: "unknown-label",
                        message: format!("unknown node label `{label}`"),
                    });
                }
            }
        }
        Expr::Subscript { base, index } => {
            check_expr(base, bindings, schema, issues);
            check_expr(index, bindings, schema, issues);
        }
        Expr::Slice { base, start, end } => {
            check_expr(base, bindings, schema, issues);
            if let Some(start) = start {
                check_expr(start, bindings, schema, issues);
            }
            if let Some(end) = end {
                check_expr(end, bindings, schema, issues);
            }
        }
        Expr::MapProjection { base, items } => {
            check_expr(base, bindings, schema, issues);
            for item in items {
                match item {
                    MapProjectionItem::Literal { value, .. } => {
                        check_expr(value, bindings, schema, issues);
                    }
                    MapProjectionItem::Variable(variable) => {
                        check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
                    }
                    MapProjectionItem::Property(_) | MapProjectionItem::AllProperties => {}
                }
            }
        }
        Expr::FunctionCall {
            name, arguments, ..
        } => {
            if let FunctionArguments::Expressions(arguments) = arguments {
                for argument in arguments {
                    check_expr(argument, bindings, schema, issues);
                }
            }
            check_function_signature(name, arguments, schema, issues);
        }
        Expr::ListComprehension {
            variable,
            source,
            predicate,
            projection,
        } => {
            check_expr(source, bindings, schema, issues);
            let mut local_bindings = bindings.clone();
            local_bindings.insert(variable.clone());
            let mut local_types = HashMap::new();
            local_types.insert(
                variable.clone(),
                list_item_type(&infer_expression_type_with(source, schema)),
            );
            let scoped = ScopedSchema {
                base: schema,
                variables: &local_types,
            };
            if let Some(predicate) = predicate {
                check_expr(predicate, &local_bindings, &scoped as &dyn Schema, issues);
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "collection predicate",
                    &scoped,
                    issues,
                );
            }
            if let Some(projection) = projection {
                check_expr(projection, &local_bindings, &scoped as &dyn Schema, issues);
            }
        }
        Expr::CollectionPredicate {
            variable,
            source,
            predicate,
            ..
        } => {
            check_expr(source, bindings, schema, issues);
            let mut local_bindings = bindings.clone();
            local_bindings.insert(variable.clone());
            let mut local_types = HashMap::new();
            local_types.insert(
                variable.clone(),
                list_item_type(&infer_expression_type_with(source, schema)),
            );
            let scoped = ScopedSchema {
                base: schema,
                variables: &local_types,
            };
            if let Some(predicate) = predicate {
                check_expr(predicate, &local_bindings, &scoped as &dyn Schema, issues);
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "collection predicate",
                    &scoped,
                    issues,
                );
            }
        }
        Expr::PatternComprehension {
            path_variable,
            pattern,
            predicate,
            projection,
        } => {
            let mut local_bindings = bindings.clone();
            add_pattern_bindings(pattern, &mut local_bindings);
            if let Some(path_variable) = path_variable {
                local_bindings.insert(path_variable.clone());
            }
            check_pattern(pattern, &local_bindings, schema, issues);
            let mut local_types = HashMap::new();
            add_pattern_types(pattern, &mut local_types);
            if let Some(path_variable) = path_variable {
                local_types.insert(path_variable.clone(), CypherType::Path);
            }
            let scoped = ScopedSchema {
                base: schema,
                variables: &local_types,
            };
            if let Some(predicate) = predicate {
                check_expr(predicate, &local_bindings, &scoped as &dyn Schema, issues);
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "pattern predicate",
                    &scoped,
                    issues,
                );
            }
            check_expr(projection, &local_bindings, &scoped as &dyn Schema, issues);
        }
        Expr::PatternExpression { pattern } => {
            check_pattern(pattern, bindings, schema, issues);
            check_pattern_expression_bindings(pattern, bindings, schema, issues);
        }
        Expr::Filter {
            variable,
            source,
            predicate,
        } => {
            check_expr(source, bindings, schema, issues);
            let mut local_bindings = bindings.clone();
            local_bindings.insert(variable.clone());
            let mut local_types = HashMap::new();
            local_types.insert(
                variable.clone(),
                list_item_type(&infer_expression_type_with(source, schema)),
            );
            let scoped = ScopedSchema {
                base: schema,
                variables: &local_types,
            };
            if let Some(predicate) = predicate {
                check_expr(predicate, &local_bindings, &scoped as &dyn Schema, issues);
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "collection predicate",
                    &scoped,
                    issues,
                );
            }
        }
        Expr::Extract {
            variable,
            source,
            projection,
        } => {
            check_expr(source, bindings, schema, issues);
            let mut local_bindings = bindings.clone();
            local_bindings.insert(variable.clone());
            let mut local_types = HashMap::new();
            local_types.insert(
                variable.clone(),
                list_item_type(&infer_expression_type_with(source, schema)),
            );
            let scoped = ScopedSchema {
                base: schema,
                variables: &local_types,
            };
            if let Some(projection) = projection {
                check_expr(projection, &local_bindings, &scoped as &dyn Schema, issues);
            }
        }
        Expr::Reduce {
            accumulator,
            initial,
            variable,
            source,
            expression,
        } => {
            check_expr(initial, bindings, schema, issues);
            check_expr(source, bindings, schema, issues);
            let mut local_bindings = bindings.clone();
            local_bindings.insert(accumulator.clone());
            local_bindings.insert(variable.clone());
            if let Some(expression) = expression {
                let mut local_types = HashMap::new();
                local_types.insert(
                    accumulator.clone(),
                    infer_expression_type_with(initial, schema),
                );
                local_types.insert(
                    variable.clone(),
                    list_item_type(&infer_expression_type_with(source, schema)),
                );
                let scoped = ScopedSchema {
                    base: schema,
                    variables: &local_types,
                };
                check_expr(expression, &local_bindings, &scoped as &dyn Schema, issues);
            }
        }
        Expr::Case {
            operand,
            alternatives,
            else_expr,
        } => {
            if let Some(operand) = operand {
                check_expr(operand, bindings, schema, issues);
            }
            for alternative in alternatives {
                check_expr(&alternative.when, bindings, schema, issues);
                check_expr(&alternative.then, bindings, schema, issues);
            }
            if let Some(else_expr) = else_expr {
                check_expr(else_expr, bindings, schema, issues);
            }
        }
        Expr::ComparisonChain { arguments, .. } => {
            for argument in arguments {
                check_expr(argument, bindings, schema, issues);
            }
        }
        Expr::Binary { lhs, rhs, .. } => {
            check_expr(lhs, bindings, schema, issues);
            check_expr(rhs, bindings, schema, issues);
        }
        Expr::Unary { operand, .. } => check_expr(operand, bindings, schema, issues),
        Expr::List(items) => {
            for item in items {
                check_expr(item, bindings, schema, issues);
            }
        }
        Expr::Map(entries) => {
            for (_k, v) in entries {
                check_expr(v, bindings, schema, issues);
            }
        }
        Expr::Literal(_) | Expr::Param(_) => {}
    }
    check_expr_type_node(expr, schema, issues);
}

fn check_expr_type_node<S: Schema + ?Sized>(expr: &Expr, schema: &S, issues: &mut Vec<SemIssue>) {
    match expr {
        Expr::Binary { op, lhs, rhs } => {
            let left = infer_expression_type_with(lhs, schema);
            let right = infer_expression_type_with(rhs, schema);
            match op {
                BinOp::And | BinOp::Xor | BinOp::Or => {
                    expect_known_type(&left, &CypherType::Boolean, "boolean operator", issues);
                    expect_known_type(&right, &CypherType::Boolean, "boolean operator", issues);
                }
                BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                    expect_numeric(&left, "arithmetic operator", issues);
                    expect_numeric(&right, "arithmetic operator", issues);
                }
                BinOp::Add => {
                    let valid = (left.is_numeric() && right.is_numeric())
                        || matches!((&left, &right), (CypherType::String, CypherType::String))
                        || matches!(
                            left,
                            CypherType::List(_) | CypherType::Any | CypherType::Null
                        )
                        || matches!(
                            right,
                            CypherType::List(_) | CypherType::Any | CypherType::Null
                        );
                    if !valid {
                        push_type_error(
                            "addition",
                            "compatible numeric, string, or list operands",
                            &format!("{left} and {right}"),
                            issues,
                        );
                    }
                }
                BinOp::RegexMatch | BinOp::StartsWith | BinOp::EndsWith | BinOp::Contains => {
                    expect_known_type(&left, &CypherType::String, "string operator", issues);
                    expect_known_type(&right, &CypherType::String, "string operator", issues);
                }
                BinOp::In => {
                    expect_known_type(
                        &right,
                        &CypherType::List(Box::new(CypherType::Any)),
                        "IN right operand",
                        issues,
                    );
                }
                BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                    let valid = (left.is_numeric() && right.is_numeric())
                        || matches!((&left, &right), (CypherType::String, CypherType::String))
                        || matches!(left, CypherType::Any | CypherType::Null)
                        || matches!(right, CypherType::Any | CypherType::Null);
                    if !valid {
                        push_type_error(
                            "ordered comparison",
                            "comparable operands",
                            &format!("{left} and {right}"),
                            issues,
                        );
                    }
                }
                BinOp::Eq | BinOp::Neq => {}
            }
        }
        Expr::Unary { op, operand } => match op {
            UnOp::Not => expect_type_with(operand, &CypherType::Boolean, "NOT", schema, issues),
            UnOp::Pos | UnOp::Neg => expect_numeric(
                &infer_expression_type_with(operand, schema),
                "unary sign",
                issues,
            ),
            UnOp::IsNull | UnOp::IsNotNull => {}
        },
        Expr::Subscript { base, index } => {
            expect_type_with(
                index,
                &CypherType::Integer,
                "subscript index",
                schema,
                issues,
            );
            let base_type = infer_expression_type_with(base, schema);
            if !matches!(
                base_type,
                CypherType::Any
                    | CypherType::Null
                    | CypherType::List(_)
                    | CypherType::Map
                    | CypherType::String
            ) {
                push_type_error(
                    "subscript",
                    "LIST, MAP, or STRING",
                    &base_type.to_string(),
                    issues,
                );
            }
        }
        Expr::Slice { base, start, end } => {
            if let Some(start) = start {
                expect_type_with(start, &CypherType::Integer, "slice start", schema, issues);
            }
            if let Some(end) = end {
                expect_type_with(end, &CypherType::Integer, "slice end", schema, issues);
            }
            let base_type = infer_expression_type_with(base, schema);
            if !matches!(
                base_type,
                CypherType::Any | CypherType::Null | CypherType::List(_) | CypherType::String
            ) {
                push_type_error("slice", "LIST or STRING", &base_type.to_string(), issues);
            }
        }
        Expr::ListComprehension {
            source, predicate, ..
        }
        | Expr::Filter {
            source, predicate, ..
        } => {
            expect_type_with(
                source,
                &CypherType::List(Box::new(CypherType::Any)),
                "collection source",
                schema,
                issues,
            );
            if let Some(predicate) = predicate {
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "collection predicate",
                    schema,
                    issues,
                );
            }
        }
        Expr::CollectionPredicate {
            source, predicate, ..
        } => {
            expect_type_with(
                source,
                &CypherType::List(Box::new(CypherType::Any)),
                "collection source",
                schema,
                issues,
            );
            if let Some(predicate) = predicate {
                expect_type_with(
                    predicate,
                    &CypherType::Boolean,
                    "collection predicate",
                    schema,
                    issues,
                );
            }
        }
        Expr::Extract { source, .. } | Expr::Reduce { source, .. } => {
            expect_type_with(
                source,
                &CypherType::List(Box::new(CypherType::Any)),
                "collection source",
                schema,
                issues,
            );
        }
        Expr::PatternComprehension {
            predicate: Some(predicate),
            ..
        } => expect_type_with(
            predicate,
            &CypherType::Boolean,
            "pattern predicate",
            schema,
            issues,
        ),
        Expr::Case {
            operand: None,
            alternatives,
            ..
        } => {
            for alternative in alternatives {
                expect_type_with(
                    &alternative.when,
                    &CypherType::Boolean,
                    "CASE WHEN",
                    schema,
                    issues,
                );
            }
        }
        _ => {}
    }
}

fn check_function_signature<S: Schema + ?Sized>(
    name: &str,
    arguments: &FunctionArguments,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    let mut signatures = schema.function_signatures(name);
    if signatures.is_empty() {
        signatures = builtin_function_signatures(name);
    }
    if signatures.is_empty() {
        return;
    }
    let FunctionArguments::Expressions(arguments) = arguments else {
        if name
            .rsplit('.')
            .next()
            .is_some_and(|part| part.eq_ignore_ascii_case("count"))
        {
            return;
        }
        issues.push(SemIssue {
            severity: SemSeverity::Error,
            code: "function-arity",
            message: format!("function `{name}` does not accept a wildcard argument"),
        });
        return;
    };
    let arity_matches = signatures
        .iter()
        .filter(|signature| signature_accepts_arity(signature, arguments.len()))
        .collect::<Vec<_>>();
    if arity_matches.is_empty() {
        let expected = signatures
            .iter()
            .map(signature_arity_description)
            .collect::<Vec<_>>()
            .join(" or ");
        issues.push(SemIssue {
            severity: SemSeverity::Error,
            code: "function-arity",
            message: format!(
                "function `{name}` expects {expected} arguments, found {}",
                arguments.len()
            ),
        });
        return;
    }
    let actual = arguments
        .iter()
        .map(|argument| infer_expression_type_with(argument, schema))
        .collect::<Vec<_>>();
    if arity_matches.iter().any(|signature| {
        actual.iter().enumerate().all(|(index, actual)| {
            signature_argument(signature, index).is_some_and(|expected| expected.accepts(actual))
        })
    }) {
        return;
    }
    push_type_error(
        &format!("arguments of function `{name}`"),
        "one of its overload signatures",
        &actual
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        issues,
    );
}

fn signature_minimum(signature: &FunctionSignature) -> usize {
    if signature.variadic && signature.arguments.len() > 1 {
        signature.arguments.len() - 1
    } else {
        signature.arguments.len()
    }
}

fn signature_accepts_arity(signature: &FunctionSignature, count: usize) -> bool {
    count >= signature_minimum(signature)
        && (signature.variadic || count == signature.arguments.len())
}

fn signature_arity_description(signature: &FunctionSignature) -> String {
    let minimum = signature_minimum(signature);
    if signature.variadic {
        format!("{minimum} or more")
    } else {
        minimum.to_string()
    }
}

fn signature_argument(signature: &FunctionSignature, index: usize) -> Option<&CypherType> {
    signature.arguments.get(index).or_else(|| {
        signature
            .variadic
            .then(|| signature.arguments.last())
            .flatten()
    })
}

fn expect_type_with<S: Schema + ?Sized>(
    expr: &Expr,
    expected: &CypherType,
    context: &str,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    expect_known_type(
        &infer_expression_type_with(expr, schema),
        expected,
        context,
        issues,
    );
}

fn expect_known_type(
    actual: &CypherType,
    expected: &CypherType,
    context: &str,
    issues: &mut Vec<SemIssue>,
) {
    if !expected.accepts(actual) {
        push_type_error(context, &expected.to_string(), &actual.to_string(), issues);
    }
}

fn expect_numeric(actual: &CypherType, context: &str, issues: &mut Vec<SemIssue>) {
    if !actual.is_numeric() && !matches!(actual, CypherType::Null) {
        push_type_error(context, "INTEGER or FLOAT", &actual.to_string(), issues);
    }
}

fn push_type_error(context: &str, expected: &str, actual: &str, issues: &mut Vec<SemIssue>) {
    issues.push(SemIssue {
        severity: SemSeverity::Error,
        code: "type-mismatch",
        message: format!("{context} requires {expected}, found {actual}"),
    });
}

fn check_pattern_expression_bindings<S: Schema + ?Sized>(
    pattern: &Pattern,
    bindings: &HashSet<String>,
    schema: &S,
    issues: &mut Vec<SemIssue>,
) {
    if let Some(variable) = &pattern.anchor.var {
        check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
    }
    for chain in &pattern.chain {
        if let Some(variable) = &chain.rel.var {
            check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
        }
        if let Some(variable) = &chain.node.var {
            check_expr(&Expr::Variable(variable.clone()), bindings, schema, issues);
        }
    }
}

/// Infer an expression type without external schema metadata.
pub fn infer_expression_type(expr: &Expr) -> CypherType {
    infer_expression_type_with(expr, &PermissiveSchema)
}

/// Infer an expression type using variable, property, parameter, and function
/// metadata supplied by `schema`.
pub fn infer_expression_type_with<S: Schema + ?Sized>(expr: &Expr, schema: &S) -> CypherType {
    match expr {
        Expr::Literal(Literal::Int(_)) => CypherType::Integer,
        Expr::Literal(Literal::Float(_)) => CypherType::Float,
        Expr::Literal(Literal::String(_)) => CypherType::String,
        Expr::Literal(Literal::Bool(_)) => CypherType::Boolean,
        Expr::Literal(Literal::Null) => CypherType::Null,
        Expr::List(items) => CypherType::List(Box::new(unify_types(
            items
                .iter()
                .map(|item| infer_expression_type_with(item, schema)),
        ))),
        Expr::Map(_) | Expr::MapProjection { .. } => CypherType::Map,
        Expr::Variable(name) => schema.variable_type(name).unwrap_or(CypherType::Any),
        Expr::Param(name) => schema.parameter_type(name).unwrap_or(CypherType::Any),
        Expr::Property { base, key } => schema
            .property_type(root_variable(base), key)
            .or_else(|| intrinsic_property_type(base, key, schema))
            .unwrap_or(CypherType::Any),
        Expr::LabelPredicate { .. }
        | Expr::CollectionPredicate { .. }
        | Expr::PatternExpression { .. }
        | Expr::ComparisonChain { .. } => CypherType::Boolean,
        Expr::Subscript { base, .. } => match infer_expression_type_with(base, schema) {
            CypherType::List(item) => *item,
            CypherType::String => CypherType::String,
            _ => CypherType::Any,
        },
        Expr::Slice { base, .. } => match infer_expression_type_with(base, schema) {
            list @ CypherType::List(_) => list,
            CypherType::String => CypherType::String,
            _ => CypherType::Any,
        },
        Expr::ListComprehension { projection, .. } => CypherType::List(Box::new(
            projection
                .as_deref()
                .map(|expr| infer_expression_type_with(expr, schema))
                .unwrap_or(CypherType::Any),
        )),
        Expr::PatternComprehension { projection, .. } => {
            CypherType::List(Box::new(infer_expression_type_with(projection, schema)))
        }
        Expr::Filter { source, .. } => match infer_expression_type_with(source, schema) {
            list @ CypherType::List(_) => list,
            _ => CypherType::List(Box::new(CypherType::Any)),
        },
        Expr::Extract { projection, .. } => CypherType::List(Box::new(
            projection
                .as_deref()
                .map(|expr| infer_expression_type_with(expr, schema))
                .unwrap_or(CypherType::Any),
        )),
        Expr::Reduce {
            initial,
            expression,
            ..
        } => expression
            .as_deref()
            .map(|expr| infer_expression_type_with(expr, schema))
            .unwrap_or_else(|| infer_expression_type_with(initial, schema)),
        Expr::Case {
            alternatives,
            else_expr,
            ..
        } => unify_types(
            alternatives
                .iter()
                .map(|alternative| infer_expression_type_with(&alternative.then, schema))
                .chain(
                    else_expr
                        .iter()
                        .map(|expr| infer_expression_type_with(expr, schema)),
                ),
        ),
        Expr::FunctionCall {
            name, arguments, ..
        } => infer_function_type(name, arguments, schema),
        Expr::Binary { op, lhs, rhs } => infer_binary_type(*op, lhs, rhs, schema),
        Expr::Unary { op, operand } => match op {
            UnOp::Not | UnOp::IsNull | UnOp::IsNotNull => CypherType::Boolean,
            UnOp::Pos | UnOp::Neg => infer_expression_type_with(operand, schema),
        },
    }
}

fn root_variable(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Variable(name) => Some(name),
        Expr::Property { base, .. } => root_variable(base),
        _ => None,
    }
}

fn intrinsic_property_type<S: Schema + ?Sized>(
    base: &Expr,
    key: &str,
    schema: &S,
) -> Option<CypherType> {
    if infer_expression_type_with(base, schema) != CypherType::Point {
        return None;
    }
    match key.to_ascii_lowercase().as_str() {
        "x" | "y" | "z" | "longitude" | "latitude" | "height" => Some(CypherType::Float),
        "srid" => Some(CypherType::Integer),
        "crs" => Some(CypherType::String),
        _ => None,
    }
}

fn unify_types(types: impl IntoIterator<Item = CypherType>) -> CypherType {
    let mut result = CypherType::Null;
    for ty in types {
        result = match (&result, &ty) {
            (CypherType::Null, _) => ty,
            (_, CypherType::Null) => result,
            (CypherType::Integer, CypherType::Float) | (CypherType::Float, CypherType::Integer) => {
                CypherType::Float
            }
            (left, right) if left == right => result,
            _ => CypherType::Any,
        };
    }
    result
}

fn infer_binary_type<S: Schema + ?Sized>(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    schema: &S,
) -> CypherType {
    match op {
        BinOp::Eq
        | BinOp::Neq
        | BinOp::Lt
        | BinOp::Lte
        | BinOp::Gt
        | BinOp::Gte
        | BinOp::And
        | BinOp::Xor
        | BinOp::Or
        | BinOp::RegexMatch
        | BinOp::In
        | BinOp::StartsWith
        | BinOp::EndsWith
        | BinOp::Contains => CypherType::Boolean,
        BinOp::Div | BinOp::Pow => CypherType::Float,
        BinOp::Add => unify_types([
            infer_expression_type_with(lhs, schema),
            infer_expression_type_with(rhs, schema),
        ]),
        BinOp::Sub | BinOp::Mul | BinOp::Mod => unify_types([
            infer_expression_type_with(lhs, schema),
            infer_expression_type_with(rhs, schema),
        ]),
    }
}

fn infer_function_type<S: Schema + ?Sized>(
    name: &str,
    arguments: &FunctionArguments,
    schema: &S,
) -> CypherType {
    let signatures = schema.function_signatures(name);
    if !signatures.is_empty() {
        if let FunctionArguments::Expressions(arguments) = arguments {
            let actual = arguments
                .iter()
                .map(|argument| infer_expression_type_with(argument, schema))
                .collect::<Vec<_>>();
            let matching = signatures.iter().filter(|signature| {
                signature_accepts_arity(signature, actual.len())
                    && actual.iter().enumerate().all(|(index, actual)| {
                        signature_argument(signature, index)
                            .is_some_and(|expected| expected.accepts(actual))
                    })
            });
            let result = unify_types(matching.map(|signature| signature.result.clone()));
            if result != CypherType::Null {
                return result;
            }
        }
        return unify_types(signatures.into_iter().map(|signature| signature.result));
    }
    let name = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
    match name.as_str() {
        "count" | "size" | "length" | "id" | "tointeger" => CypherType::Integer,
        "avg" | "tofloat" => CypherType::Float,
        "tostring" | "type" => CypherType::String,
        "exists" => CypherType::Boolean,
        "labels" | "keys" => CypherType::List(Box::new(CypherType::String)),
        "range" => CypherType::List(Box::new(CypherType::Integer)),
        "collect" => CypherType::List(Box::new(match arguments {
            FunctionArguments::Expressions(arguments) => arguments
                .first()
                .map(|argument| infer_expression_type_with(argument, schema))
                .unwrap_or(CypherType::Any),
            FunctionArguments::Wildcard => CypherType::Any,
        })),
        "sum" | "min" | "max" | "head" | "last" | "coalesce" => match arguments {
            FunctionArguments::Expressions(arguments) => unify_types(
                arguments
                    .iter()
                    .map(|argument| infer_expression_type_with(argument, schema)),
            ),
            FunctionArguments::Wildcard => CypherType::Any,
        },
        _ => infer_builtin_overload_result(&name, arguments, schema),
    }
}

fn builtin_function_signatures(name: &str) -> Vec<FunctionSignature> {
    let Some(name) = name.rsplit('.').next() else {
        return Vec::new();
    };
    let name = name.to_ascii_lowercase();
    let one_any = |result| FunctionSignature {
        arguments: vec![CypherType::Any],
        variadic: false,
        result,
    };
    let exact = |arguments, result| FunctionSignature {
        arguments,
        variadic: false,
        result,
    };
    let numeric_to_float = || {
        vec![
            exact(vec![CypherType::Integer], CypherType::Float),
            exact(vec![CypherType::Float], CypherType::Float),
        ]
    };
    let signatures = match name.as_str() {
        "count" => vec![one_any(CypherType::Integer)],
        "collect" => vec![one_any(CypherType::List(Box::new(CypherType::Any)))],
        "size" => vec![
            exact(vec![CypherType::String], CypherType::Integer),
            exact(
                vec![CypherType::List(Box::new(CypherType::Any))],
                CypherType::Integer,
            ),
            exact(vec![CypherType::Map], CypherType::Integer),
        ],
        "length" => vec![
            exact(vec![CypherType::Path], CypherType::Integer),
            exact(vec![CypherType::String], CypherType::Integer),
            exact(
                vec![CypherType::List(Box::new(CypherType::Any))],
                CypherType::Integer,
            ),
        ],
        "id" => vec![
            exact(vec![CypherType::Node], CypherType::Integer),
            exact(vec![CypherType::Relationship], CypherType::Integer),
        ],
        "tointeger" => vec![one_any(CypherType::Integer)],
        "avg" => vec![
            exact(vec![CypherType::Integer], CypherType::Float),
            exact(vec![CypherType::Float], CypherType::Float),
        ],
        "tofloat" => vec![one_any(CypherType::Float)],
        "tostring" => vec![one_any(CypherType::String)],
        "toboolean" => vec![one_any(CypherType::Boolean)],
        "type" => vec![exact(vec![CypherType::Relationship], CypherType::String)],
        "exists" => vec![one_any(CypherType::Boolean)],
        "labels" => vec![exact(
            vec![CypherType::Node],
            CypherType::List(Box::new(CypherType::String)),
        )],
        "keys" => vec![
            exact(
                vec![CypherType::Map],
                CypherType::List(Box::new(CypherType::String)),
            ),
            exact(
                vec![CypherType::Node],
                CypherType::List(Box::new(CypherType::String)),
            ),
            exact(
                vec![CypherType::Relationship],
                CypherType::List(Box::new(CypherType::String)),
            ),
        ],
        "properties" => vec![
            exact(vec![CypherType::Map], CypherType::Map),
            exact(vec![CypherType::Node], CypherType::Map),
            exact(vec![CypherType::Relationship], CypherType::Map),
        ],
        "point" => vec![exact(vec![CypherType::Map], CypherType::Point)],
        "distance" => vec![exact(
            vec![CypherType::Point, CypherType::Point],
            CypherType::Float,
        )],
        "startnode" | "endnode" => vec![exact(vec![CypherType::Relationship], CypherType::Node)],
        "nodes" => vec![exact(
            vec![CypherType::Path],
            CypherType::List(Box::new(CypherType::Node)),
        )],
        "relationships" => vec![exact(
            vec![CypherType::Path],
            CypherType::List(Box::new(CypherType::Relationship)),
        )],
        "tail" => vec![exact(
            vec![CypherType::List(Box::new(CypherType::Any))],
            CypherType::List(Box::new(CypherType::Any)),
        )],
        "reverse" => vec![
            exact(vec![CypherType::String], CypherType::String),
            exact(
                vec![CypherType::List(Box::new(CypherType::Any))],
                CypherType::List(Box::new(CypherType::Any)),
            ),
        ],
        "sum" => vec![
            exact(vec![CypherType::Integer], CypherType::Integer),
            exact(vec![CypherType::Float], CypherType::Float),
        ],
        "min" | "max" => vec![one_any(CypherType::Any)],
        "stdev" | "stdevp" => numeric_to_float(),
        "percentilecont" | "percentiledisc" => vec![
            exact(
                vec![CypherType::Integer, CypherType::Integer],
                CypherType::Float,
            ),
            exact(
                vec![CypherType::Integer, CypherType::Float],
                CypherType::Float,
            ),
            exact(
                vec![CypherType::Float, CypherType::Integer],
                CypherType::Float,
            ),
            exact(
                vec![CypherType::Float, CypherType::Float],
                CypherType::Float,
            ),
        ],
        "head" | "last" => vec![FunctionSignature {
            arguments: vec![CypherType::List(Box::new(CypherType::Any))],
            variadic: false,
            result: CypherType::Any,
        }],
        "coalesce" => vec![FunctionSignature {
            arguments: vec![CypherType::Any],
            variadic: true,
            result: CypherType::Any,
        }],
        "range" => vec![
            FunctionSignature {
                arguments: vec![CypherType::Integer, CypherType::Integer],
                variadic: false,
                result: CypherType::List(Box::new(CypherType::Integer)),
            },
            FunctionSignature {
                arguments: vec![
                    CypherType::Integer,
                    CypherType::Integer,
                    CypherType::Integer,
                ],
                variadic: false,
                result: CypherType::List(Box::new(CypherType::Integer)),
            },
        ],
        "timestamp" => vec![exact(Vec::new(), CypherType::Integer)],
        "abs" => vec![
            exact(vec![CypherType::Integer], CypherType::Integer),
            exact(vec![CypherType::Float], CypherType::Float),
        ],
        "ceil" | "floor" | "round" => numeric_to_float(),
        "sign" => vec![
            exact(vec![CypherType::Integer], CypherType::Integer),
            exact(vec![CypherType::Float], CypherType::Integer),
        ],
        "rand" | "e" | "pi" => vec![exact(Vec::new(), CypherType::Float)],
        "exp" | "log" | "log10" | "sqrt" | "acos" | "asin" | "atan" | "cos" | "cot" | "degrees"
        | "haversin" | "radians" | "sin" | "tan" => numeric_to_float(),
        "atan2" => vec![
            exact(
                vec![CypherType::Integer, CypherType::Integer],
                CypherType::Float,
            ),
            exact(
                vec![CypherType::Integer, CypherType::Float],
                CypherType::Float,
            ),
            exact(
                vec![CypherType::Float, CypherType::Integer],
                CypherType::Float,
            ),
            exact(
                vec![CypherType::Float, CypherType::Float],
                CypherType::Float,
            ),
        ],
        "left" | "right" => vec![exact(
            vec![CypherType::String, CypherType::Integer],
            CypherType::String,
        )],
        "ltrim" | "rtrim" | "trim" | "tolower" | "toupper" => {
            vec![exact(vec![CypherType::String], CypherType::String)]
        }
        "replace" => vec![exact(
            vec![CypherType::String, CypherType::String, CypherType::String],
            CypherType::String,
        )],
        "split" => vec![exact(
            vec![CypherType::String, CypherType::String],
            CypherType::List(Box::new(CypherType::String)),
        )],
        "substring" => vec![
            exact(
                vec![CypherType::String, CypherType::Integer],
                CypherType::String,
            ),
            exact(
                vec![CypherType::String, CypherType::Integer, CypherType::Integer],
                CypherType::String,
            ),
        ],
        _ => Vec::new(),
    };
    signatures
}
