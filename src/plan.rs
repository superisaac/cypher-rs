//! Logical query plan and AST-to-plan lowering.
//!
//! v0.4 introduced a small algebra (`Scan` / `Expand` / `Filter` /
//! `Project` / `Sort` / `Skip` / `Limit`) and lowered single-MATCH /
//! single-pattern queries.
//!
//! v0.5 extends that:
//!   - **Multi-pattern MATCH** (`MATCH (a), (b)`) lowers to a
//!     `Cartesian` of per-pattern lowerings.
//!   - **Multiple MATCH clauses** lower to a `Cartesian` chain
//!     (left-deep).
//!   - **OPTIONAL MATCH** lowers to `Optional`, a left-outer apply
//!     that emits a row from the input even when the optional plan
//!     produces nothing.
//!
//! No optimization, no cost model. The plan is data, not code -
//! print it, serialize it, optimize it, send it across a wire. See
//! the [`std::fmt::Display`] impl for the indented tree rendering.

use std::fmt;

use crate::ast::*;

/// Logical plan operator. Plans are trees; every operator carries
/// its input(s).
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Empty input. Used for queries with no `MATCH` (e.g. `RETURN 1`).
    Empty,
    /// Scan all nodes (optionally filtered by `label`), binding them
    /// to `var`.
    Scan {
        var: Option<String>,
        label: Option<String>,
    },
    /// Extend each row by following a relationship from `src` to `dst`.
    Expand {
        input: Box<Plan>,
        src: Option<String>,
        rel_var: Option<String>,
        rel_types: Vec<String>,
        range: Option<RelationshipRange>,
        direction: Direction,
        dst: Option<String>,
        dst_label: Option<String>,
    },
    /// Keep only rows where `pred` evaluates to true.
    Filter { input: Box<Plan>, pred: Expr },
    /// Match an entity's properties against a complete map supplied at runtime.
    PropertyMapFilter {
        input: Box<Plan>,
        variable: String,
        map: Expr,
    },
    /// Replace columns with the projected expressions.
    Project {
        input: Box<Plan>,
        include_existing: bool,
        exprs: Vec<ProjectExpr>,
    },
    /// Sort rows by `keys` (left-to-right priority).
    Sort {
        input: Box<Plan>,
        keys: Vec<SortKey>,
    },
    /// Discard the first `count` rows.
    Skip { input: Box<Plan>, count: Expr },
    /// Keep at most `count` rows after any prior `Skip`.
    Limit { input: Box<Plan>, count: Expr },
    /// Cartesian product of two plan trees. Left-deep by convention
    /// (the lowerer reduces a list of patterns left-to-right).
    Cartesian { left: Box<Plan>, right: Box<Plan> },
    /// Outer-apply: for each row from `input`, evaluate `optional`.
    /// If `optional` produces rows, emit them. If it produces nothing,
    /// emit one row from `input` with the optional bindings as null.
    Optional {
        input: Box<Plan>,
        optional: Box<Plan>,
    },
    /// Select shortest paths from an expanded pattern path.
    ShortestPath { input: Box<Plan>, all: bool },
    /// Bind the complete path produced by the input pattern to `variable`.
    NamedPath { input: Box<Plan>, variable: String },
    /// Preserve a planner directive attached to a MATCH clause.
    PlannerHint { input: Box<Plan>, hint: MatchHint },
    /// Invoke a procedure for each input row and append its yielded fields.
    ProcedureCall {
        input: Box<Plan>,
        name: String,
        arguments: Vec<Expr>,
        yields: Vec<YieldItem>,
    },
    /// Read CSV records for each input row and bind each record to `variable`.
    LoadCsv {
        input: Box<Plan>,
        with_headers: bool,
        url: Expr,
        variable: String,
        field_terminator: Option<String>,
    },
    /// Create graph entities described by `patterns` for every input row.
    Create {
        input: Box<Plan>,
        unique: bool,
        patterns: Vec<Pattern>,
    },
    /// Match an existing pattern or create it, then apply the corresponding
    /// `ON MATCH` / `ON CREATE` updates for every input row.
    Merge {
        input: Box<Plan>,
        pattern: Pattern,
        actions: Vec<MergeAction>,
    },
    /// Update properties or labels for every input row.
    Set {
        input: Box<Plan>,
        items: Vec<SetItem>,
    },
    /// Remove properties or labels for every input row.
    Remove {
        input: Box<Plan>,
        items: Vec<RemoveItem>,
    },
    /// Execute the query in transaction batches of an optional explicit size.
    PeriodicCommit {
        input: Box<Plan>,
        limit: Option<u64>,
    },
    /// Return execution-plan information instead of executing the query.
    Explain { input: Box<Plan> },
    /// Execute the query while collecting runtime profile information.
    Profile { input: Box<Plan> },
    /// Remove duplicate rows from `input`. Corresponds to `RETURN DISTINCT`
    /// or `WITH DISTINCT` in openCypher.
    Distinct { input: Box<Plan> },
    /// Combine rows from two independently planned query branches.
    /// Plain `UNION` removes duplicates; `UNION ALL` preserves them.
    Union {
        left: Box<Plan>,
        right: Box<Plan>,
        all: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectExpr {
    pub expr: Expr,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SortKey {
    pub expr: Expr,
    pub desc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The query had no clauses at all.
    EmptyQuery,
    /// The logical-plan API returns one plan and cannot represent a batch.
    MultipleStatementsUnsupported,
    /// `OPTIONAL MATCH` requires a prior plan to attach to.
    OptionalMatchWithoutAnchor,
    /// Every UNION branch must end in a projection.
    UnionBranchWithoutReturn,
    /// UNION branches must project the same number of columns.
    UnionColumnCountMismatch { left: usize, right: usize },
    /// The parser supports the clause, but the read-only logical planner does not.
    UnsupportedClause(&'static str),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::EmptyQuery => f.write_str("plan: empty query"),
            PlanError::MultipleStatementsUnsupported => {
                f.write_str("plan: multiple statements are not supported")
            }
            PlanError::OptionalMatchWithoutAnchor => {
                f.write_str("plan: OPTIONAL MATCH must follow at least one regular MATCH clause")
            }
            PlanError::UnionBranchWithoutReturn => {
                f.write_str("plan: every UNION branch must contain a RETURN clause")
            }
            PlanError::UnionColumnCountMismatch { left, right } => write!(
                f,
                "plan: UNION branches project different column counts ({left} and {right})"
            ),
            PlanError::UnsupportedClause(clause) => {
                write!(f, "plan: {clause} clauses are not supported")
            }
        }
    }
}

impl std::error::Error for PlanError {}

/// Lower a parsed query into a logical plan tree.
pub fn plan(query: &Query) -> Result<Plan, PlanError> {
    if !query.additional_statements.is_empty() {
        return Err(PlanError::MultipleStatementsUnsupported);
    }
    if query.clauses.is_empty() {
        return Err(PlanError::EmptyQuery);
    }

    let mut branches = Vec::new();
    let mut operators = Vec::new();
    let mut branch_start = 0;
    for (index, clause) in query.clauses.iter().enumerate() {
        if let Clause::Union(union) = clause {
            branches.push(plan_branch(&query.clauses[branch_start..index])?);
            operators.push(*union);
            branch_start = index + 1;
        }
    }
    branches.push(plan_branch(&query.clauses[branch_start..])?);

    if operators.is_empty() {
        let plan = branches.pop().expect("one branch").0;
        return Ok(apply_query_options(plan, &query.options));
    }

    let expected_columns = branches[0].1.ok_or(PlanError::UnionBranchWithoutReturn)?;
    for (_, columns) in branches.iter().skip(1) {
        let columns = columns.ok_or(PlanError::UnionBranchWithoutReturn)?;
        if columns != expected_columns {
            return Err(PlanError::UnionColumnCountMismatch {
                left: expected_columns,
                right: columns,
            });
        }
    }

    let mut plans = branches.into_iter().map(|(plan, _)| plan);
    let mut combined = plans.next().ok_or(PlanError::EmptyQuery)?;
    for (right, union) in plans.zip(operators) {
        combined = Plan::Union {
            left: Box::new(combined),
            right: Box::new(right),
            all: union.all,
        };
    }
    Ok(apply_query_options(combined, &query.options))
}

fn apply_query_options(mut plan: Plan, options: &[QueryOption]) -> Plan {
    for option in options.iter().rev() {
        match option {
            QueryOption::Explain => {
                plan = Plan::Explain {
                    input: Box::new(plan),
                };
            }
            QueryOption::Profile => {
                plan = Plan::Profile {
                    input: Box::new(plan),
                };
            }
            QueryOption::Cypher { .. } => {}
            QueryOption::UsingPeriodicCommit { limit } => {
                plan = Plan::PeriodicCommit {
                    input: Box::new(plan),
                    limit: *limit,
                };
            }
        }
    }
    plan
}

fn plan_branch(clauses: &[Clause]) -> Result<(Plan, Option<usize>), PlanError> {
    let mut plan = Plan::Empty;
    let mut project: Option<(bool, Vec<ProjectExpr>)> = None;
    let mut return_distinct = false;
    let mut visible_bindings = std::collections::HashSet::new();
    let mut return_columns = None;
    let mut sort: Option<Vec<SortKey>> = None;
    let mut skip: Option<Expr> = None;
    let mut limit: Option<Expr> = None;

    for clause in clauses {
        match clause {
            Clause::SchemaCommand(command) => {
                return Err(PlanError::UnsupportedClause(command.name()));
            }
            Clause::Match(m) => {
                collect_match_bindings(m, &mut visible_bindings);
                let lowered = lower_match(m)?;
                if m.optional {
                    if matches!(plan, Plan::Empty) {
                        return Err(PlanError::OptionalMatchWithoutAnchor);
                    }
                    plan = Plan::Optional {
                        input: Box::new(plan),
                        optional: Box::new(lowered),
                    };
                } else {
                    plan = match plan {
                        Plan::Empty => lowered,
                        existing => Plan::Cartesian {
                            left: Box::new(existing),
                            right: Box::new(lowered),
                        },
                    };
                }
            }
            Clause::Where(e) => {
                plan = Plan::Filter {
                    input: Box::new(plan),
                    pred: e.clone(),
                };
            }
            Clause::Call(call) => {
                visible_bindings.extend(call.yields.iter().map(|item| item.binding().to_string()));
                plan = Plan::ProcedureCall {
                    input: Box::new(plan),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                    yields: call.yields.clone(),
                };
                if let Some(predicate) = &call.predicate {
                    plan = Plan::Filter {
                        input: Box::new(plan),
                        pred: predicate.clone(),
                    };
                }
            }
            Clause::LoadCsv(load_csv) => {
                visible_bindings.insert(load_csv.variable.clone());
                plan = Plan::LoadCsv {
                    input: Box::new(plan),
                    with_headers: load_csv.with_headers,
                    url: load_csv.url.clone(),
                    variable: load_csv.variable.clone(),
                    field_terminator: load_csv.field_terminator.clone(),
                };
            }
            Clause::With(r) => {
                let exprs = r
                    .items
                    .iter()
                    .map(|i| ProjectExpr {
                        expr: i.expr.clone(),
                        alias: i.alias.clone(),
                    })
                    .collect();
                plan = Plan::Project {
                    input: Box::new(plan),
                    include_existing: r.include_existing,
                    exprs,
                };
                if !r.include_existing {
                    visible_bindings.clear();
                }
                visible_bindings.extend(r.items.iter().filter_map(return_item_name));
                if r.distinct {
                    plan = Plan::Distinct {
                        input: Box::new(plan),
                    };
                }
            }
            Clause::Return(r) => {
                return_distinct = r.distinct;
                return_columns = Some(
                    r.items.len()
                        + if r.include_existing {
                            visible_bindings.len()
                        } else {
                            0
                        },
                );
                project = Some((
                    r.include_existing,
                    r.items
                        .iter()
                        .map(|i| ProjectExpr {
                            expr: i.expr.clone(),
                            alias: i.alias.clone(),
                        })
                        .collect(),
                ));
            }
            Clause::OrderBy(items) => {
                sort = Some(
                    items
                        .iter()
                        .map(|i| SortKey {
                            expr: i.expr.clone(),
                            desc: i.desc,
                        })
                        .collect(),
                );
            }
            Clause::Skip(e) => skip = Some(e.clone()),
            Clause::Limit(e) => limit = Some(e.clone()),
            Clause::Create(create) => {
                for pattern in &create.patterns {
                    collect_pattern_bindings(pattern, &mut visible_bindings);
                }
                plan = Plan::Create {
                    input: Box::new(plan),
                    unique: create.unique,
                    patterns: create.patterns.clone(),
                };
            }
            Clause::Merge(merge) => {
                collect_pattern_bindings(&merge.pattern, &mut visible_bindings);
                plan = Plan::Merge {
                    input: Box::new(plan),
                    pattern: merge.pattern.clone(),
                    actions: merge.actions.clone(),
                };
            }
            Clause::Set(set) => {
                plan = Plan::Set {
                    input: Box::new(plan),
                    items: set.items.clone(),
                };
            }
            Clause::Remove(remove) => {
                plan = Plan::Remove {
                    input: Box::new(plan),
                    items: remove.items.clone(),
                };
            }
            Clause::Delete(_) => return Err(PlanError::UnsupportedClause("DELETE")),
            Clause::Unwind(_) => return Err(PlanError::UnsupportedClause("UNWIND")),
            Clause::Foreach(_) => return Err(PlanError::UnsupportedClause("FOREACH")),
            Clause::Start(_) => return Err(PlanError::UnsupportedClause("START")),
            Clause::Union(_) => unreachable!("UNION markers are split before branch planning"),
        }
    }

    // Stack post-RETURN clauses on top: project → distinct? → sort → skip → limit.
    if let Some((include_existing, exprs)) = project {
        plan = Plan::Project {
            input: Box::new(plan),
            include_existing,
            exprs,
        };
        if return_distinct {
            plan = Plan::Distinct {
                input: Box::new(plan),
            };
        }
    }
    if let Some(keys) = sort {
        plan = Plan::Sort {
            input: Box::new(plan),
            keys,
        };
    }
    if let Some(count) = skip {
        plan = Plan::Skip {
            input: Box::new(plan),
            count,
        };
    }
    if let Some(count) = limit {
        plan = Plan::Limit {
            input: Box::new(plan),
            count,
        };
    }

    Ok((plan, return_columns))
}

fn collect_match_bindings(clause: &MatchClause, bindings: &mut std::collections::HashSet<String>) {
    for pattern in &clause.patterns {
        collect_pattern_bindings(pattern, bindings);
    }
}

fn collect_pattern_bindings(pattern: &Pattern, bindings: &mut std::collections::HashSet<String>) {
    bindings.extend(pattern.path_variable.iter().cloned());
    bindings.extend(pattern.anchor.var.iter().cloned());
    for chain in &pattern.chain {
        bindings.extend(chain.rel.var.iter().cloned());
        bindings.extend(chain.node.var.iter().cloned());
    }
}

fn return_item_name(item: &ReturnItem) -> Option<String> {
    item.alias.clone().or_else(|| match &item.expr {
        Expr::Variable(variable) => Some(variable.clone()),
        _ => None,
    })
}

fn lower_match(m: &MatchClause) -> Result<Plan, PlanError> {
    if m.patterns.is_empty() {
        return Err(PlanError::EmptyQuery);
    }
    let mut iter = m.patterns.iter();
    let first = iter.next().expect("non-empty patterns");
    let mut combined = lower_pattern(first);
    for pattern in iter {
        combined = Plan::Cartesian {
            left: Box::new(combined),
            right: Box::new(lower_pattern(pattern)),
        };
    }
    for hint in &m.hints {
        combined = Plan::PlannerHint {
            input: Box::new(combined),
            hint: hint.clone(),
        };
    }
    Ok(combined)
}

fn lower_pattern(pattern: &Pattern) -> Plan {
    let mut synth = Synth::default();

    // Anchor: synthesize a binding only if it has properties but no var.
    let anchor_var = effective_var(
        &pattern.anchor.var,
        &pattern.anchor.properties,
        pattern.anchor.property_map.is_some(),
        "node",
        &mut synth,
    );
    let mut current = Plan::Scan {
        var: anchor_var.clone(),
        label: pattern.anchor.labels.first().cloned(),
    };
    if let Some(filter) = pattern_property_filter(&anchor_var, &pattern.anchor.properties) {
        current = Plan::Filter {
            input: Box::new(current),
            pred: filter,
        };
    }
    if let (Some(variable), Some(map)) = (&anchor_var, &pattern.anchor.property_map) {
        current = Plan::PropertyMapFilter {
            input: Box::new(current),
            variable: variable.clone(),
            map: map.clone(),
        };
    }
    let mut head = anchor_var;

    for chain in &pattern.chain {
        let rel_var = effective_var(
            &chain.rel.var,
            &chain.rel.properties,
            chain.rel.property_map.is_some(),
            "rel",
            &mut synth,
        );
        let dst_var = effective_var(
            &chain.node.var,
            &chain.node.properties,
            chain.node.property_map.is_some(),
            "node",
            &mut synth,
        );

        current = Plan::Expand {
            input: Box::new(current),
            src: head.clone(),
            rel_var: rel_var.clone(),
            rel_types: chain.rel.types.clone(),
            range: chain.rel.range,
            direction: chain.rel.direction,
            dst: dst_var.clone(),
            dst_label: chain.node.labels.first().cloned(),
        };
        if let Some(filter) = pattern_property_filter(&rel_var, &chain.rel.properties) {
            current = Plan::Filter {
                input: Box::new(current),
                pred: filter,
            };
        }
        if let (Some(variable), Some(map)) = (&rel_var, &chain.rel.property_map) {
            current = Plan::PropertyMapFilter {
                input: Box::new(current),
                variable: variable.clone(),
                map: map.clone(),
            };
        }
        if let Some(filter) = pattern_property_filter(&dst_var, &chain.node.properties) {
            current = Plan::Filter {
                input: Box::new(current),
                pred: filter,
            };
        }
        if let (Some(variable), Some(map)) = (&dst_var, &chain.node.property_map) {
            current = Plan::PropertyMapFilter {
                input: Box::new(current),
                variable: variable.clone(),
                map: map.clone(),
            };
        }
        head = dst_var;
    }
    current = match pattern.shortest {
        Some(mode) => Plan::ShortestPath {
            input: Box::new(current),
            all: matches!(mode, ShortestPathMode::All),
        },
        None => current,
    };
    match &pattern.path_variable {
        Some(variable) => Plan::NamedPath {
            input: Box::new(current),
            variable: variable.clone(),
        },
        None => current,
    }
}

/// Counter for synthesized binding names, scoped to one `lower_pattern` call.
#[derive(Default)]
struct Synth {
    next: usize,
}

impl Synth {
    fn fresh(&mut self, prefix: &str) -> String {
        let name = format!("__{prefix}_{}", self.next);
        self.next += 1;
        name
    }
}

/// Returns the variable name to use for a pattern element. If the user
/// supplied a name, use it. Otherwise, if the element has properties
/// that need a binding to attach a filter to, synthesize one
/// (`__node_0`, `__rel_1`, etc.). If neither, return `None` and let
/// the operator be unbound.
fn effective_var(
    user_var: &Option<String>,
    properties: &[(String, Expr)],
    has_property_map: bool,
    prefix: &str,
    synth: &mut Synth,
) -> Option<String> {
    if let Some(v) = user_var {
        return Some(v.clone());
    }
    if properties.is_empty() && !has_property_map {
        return None;
    }
    Some(synth.fresh(prefix))
}

/// Desugar a pattern's `{key: value}` block into a single AND-chain
/// filter predicate of the form `var.key = value AND ...`.
/// Returns `None` when the pattern is unbound (no var to attach to)
/// or has no properties.
fn pattern_property_filter(var: &Option<String>, props: &[(String, Expr)]) -> Option<Expr> {
    if props.is_empty() {
        return None;
    }
    let var_name = var.as_ref()?;
    let mut iter = props.iter();
    let first = iter.next()?;
    let mut acc = property_eq(var_name, &first.0, &first.1);
    for (k, v) in iter {
        acc = Expr::Binary {
            op: BinOp::And,
            lhs: Box::new(acc),
            rhs: Box::new(property_eq(var_name, k, v)),
        };
    }
    Some(acc)
}

fn property_eq(var: &str, key: &str, value: &Expr) -> Expr {
    Expr::Binary {
        op: BinOp::Eq,
        lhs: Box::new(Expr::Property {
            base: Box::new(Expr::Variable(var.to_string())),
            key: key.to_string(),
        }),
        rhs: Box::new(value.clone()),
    }
}

// --- pretty-printing -------------------------------------------------------

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_plan(self, f, 0, true)
    }
}

fn write_plan(plan: &Plan, f: &mut fmt::Formatter<'_>, depth: usize, root: bool) -> fmt::Result {
    let indent = "    ".repeat(depth.saturating_sub(1));
    let lead = if root { "" } else { "└── " };
    write!(f, "{indent}{lead}")?;
    match plan {
        Plan::Empty => writeln!(f, "Empty")?,
        Plan::Scan { var, label } => writeln!(
            f,
            "Scan {{ var: {}, label: {} }}",
            opt_str(var.as_deref()),
            opt_str(label.as_deref()),
        )?,
        Plan::Expand {
            input,
            src,
            rel_var,
            rel_types,
            range,
            direction,
            dst,
            dst_label,
        } => {
            writeln!(
                f,
                "Expand {{ src: {}, rel: {}, types: [{}], range: {}, dir: {}, dst: {}, dst_label: {} }}",
                opt_str(src.as_deref()),
                opt_str(rel_var.as_deref()),
                rel_types.join(", "),
                range_str(*range),
                direction_str(*direction),
                opt_str(dst.as_deref()),
                opt_str(dst_label.as_deref()),
            )?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Filter { input, pred } => {
            writeln!(f, "Filter {{ pred: {pred:?} }}")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::PropertyMapFilter {
            input,
            variable,
            map,
        } => {
            writeln!(
                f,
                "PropertyMapFilter {{ variable: {variable}, map: {map:?} }}"
            )?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Project {
            input,
            include_existing,
            exprs,
        } => {
            let mut parts = Vec::new();
            if *include_existing {
                parts.push("*".to_string());
            }
            parts.extend(exprs.iter().map(|e| match &e.alias {
                Some(a) => format!("{:?} AS {a}", e.expr),
                None => format!("{:?}", e.expr),
            }));
            writeln!(f, "Project {{ exprs: [{}] }}", parts.join(", "))?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Sort { input, keys } => {
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{:?} {}", k.expr, if k.desc { "DESC" } else { "ASC" }))
                .collect();
            writeln!(f, "Sort {{ keys: [{}] }}", parts.join(", "))?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Skip { input, count } => {
            writeln!(f, "Skip {{ count: {count:?} }}")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Limit { input, count } => {
            writeln!(f, "Limit {{ count: {count:?} }}")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Cartesian { left, right } => {
            writeln!(f, "Cartesian")?;
            write_plan(left, f, depth + 1, false)?;
            write_plan(right, f, depth + 1, false)?;
        }
        Plan::Optional { input, optional } => {
            writeln!(f, "Optional")?;
            write_plan(input, f, depth + 1, false)?;
            write_plan(optional, f, depth + 1, false)?;
        }
        Plan::ShortestPath { input, all } => {
            writeln!(f, "ShortestPath {{ all: {all} }}")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::NamedPath { input, variable } => {
            writeln!(f, "NamedPath {{ variable: {variable} }}")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::PlannerHint { input, hint } => {
            writeln!(f, "PlannerHint {{ hint: {hint:?} }}")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::ProcedureCall {
            input,
            name,
            arguments,
            yields,
        } => {
            let yield_names = yields
                .iter()
                .map(|item| match &item.alias {
                    Some(alias) => format!("{} AS {alias}", item.field),
                    None => item.field.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                f,
                "ProcedureCall {{ name: {name}, args: {arguments:?}, yield: [{yield_names}] }}"
            )?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::LoadCsv {
            input,
            with_headers,
            url,
            variable,
            field_terminator,
        } => {
            writeln!(
                f,
                "LoadCsv {{ with_headers: {with_headers}, url: {url:?}, variable: {variable}, field_terminator: {} }}",
                field_terminator.as_deref().unwrap_or("_")
            )?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Create {
            input,
            unique,
            patterns,
        } => {
            writeln!(
                f,
                "Create {{ unique: {unique}, patterns: {} }}",
                patterns.len()
            )?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Merge {
            input,
            pattern: _,
            actions,
        } => {
            writeln!(f, "Merge {{ actions: {} }}", actions.len())?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Set { input, items } => {
            writeln!(f, "Set {{ items: {} }}", items.len())?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Remove { input, items } => {
            writeln!(f, "Remove {{ items: {} }}", items.len())?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::PeriodicCommit { input, limit } => {
            writeln!(
                f,
                "PeriodicCommit {{ limit: {} }}",
                limit.map_or_else(|| "_".into(), |value| value.to_string())
            )?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Explain { input } => {
            writeln!(f, "Explain")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Profile { input } => {
            writeln!(f, "Profile")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Distinct { input } => {
            writeln!(f, "Distinct")?;
            write_plan(input, f, depth + 1, false)?;
        }
        Plan::Union { left, right, all } => {
            writeln!(f, "Union {{ all: {all} }}")?;
            write_plan(left, f, depth + 1, false)?;
            write_plan(right, f, depth + 1, false)?;
        }
    }
    Ok(())
}

fn opt_str(s: Option<&str>) -> String {
    match s {
        Some(v) => v.to_string(),
        None => "_".to_string(),
    }
}

fn range_str(range: Option<RelationshipRange>) -> String {
    let Some(range) = range else {
        return "_".into();
    };
    match (range.start, range.end) {
        (None, None) => "*".into(),
        (Some(start), Some(end)) if start == end => format!("*{start}"),
        (start, end) => format!(
            "*{}..{}",
            start.map_or_else(String::new, |value| value.to_string()),
            end.map_or_else(String::new, |value| value.to_string())
        ),
    }
}

fn direction_str(d: Direction) -> &'static str {
    match d {
        Direction::Outgoing => "->",
        Direction::Incoming => "<-",
        Direction::Undirected => "--",
    }
}
