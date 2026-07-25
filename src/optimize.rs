//! Logical-plan optimizer (v0.6 / v0.11).
//!
//! Predicate pushdown moves a `Filter` as far down the tree as its
//! predicate's variable references allow, on the principle that
//! filtering early shrinks the rows that flow through later operators.
//!
//! Push directions:
//!   - `Filter(Project(input, exprs), pred)` -> `Project(Filter(input, pred), exprs)`
//!     when `pred` doesn't reference any project alias.
//!   - `Filter(Sort(input, keys), pred)` -> `Sort(Filter(input, pred), keys)`
//!     (always safe; predicate evaluation doesn't depend on order).
//!   - `Filter(Cartesian(l, r), pred)` -> `Cartesian(Filter(l, pred), r)`
//!     (or symmetric) when `pred` only references vars bound on one
//!     side.
//!   - `Filter(Expand(input, ..., rel_var, dst), pred)` ->
//!     `Expand(Filter(input, pred), ..., rel_var, dst)` when `pred`
//!     doesn't reference `rel_var` or `dst` (the vars Expand
//!     introduces). This avoids expanding rows that will be filtered
//!     immediately after.
//!
//! Push *blockers*: we don't push through `Limit`, `Skip`, or
//! `Optional`, because doing so changes which rows are seen.
//!
//! `optimize` runs the rewrite to a fixpoint. The transformation is
//! semantics-preserving: `eval(plan) == eval(optimize(plan))`.

use std::collections::HashSet;

use crate::ast::*;
use crate::plan::{Plan, ProjectExpr};

/// Apply pushdown rewrites until the plan stops changing.
pub fn optimize(plan: Plan) -> Plan {
    let mut current = plan;
    loop {
        let next = pass(current.clone());
        if next == current {
            return current;
        }
        current = next;
    }
}

fn pass(plan: Plan) -> Plan {
    let with_children = descend(plan);
    apply_local(with_children)
}

fn descend(plan: Plan) -> Plan {
    match plan {
        Plan::Filter { input, pred } => Plan::Filter {
            input: Box::new(pass(*input)),
            pred,
        },
        Plan::PropertyMapFilter {
            input,
            variable,
            map,
        } => Plan::PropertyMapFilter {
            input: Box::new(pass(*input)),
            variable,
            map,
        },
        Plan::Project {
            input,
            include_existing,
            exprs,
        } => Plan::Project {
            input: Box::new(pass(*input)),
            include_existing,
            exprs,
        },
        Plan::Sort { input, keys } => Plan::Sort {
            input: Box::new(pass(*input)),
            keys,
        },
        Plan::Skip { input, count } => Plan::Skip {
            input: Box::new(pass(*input)),
            count,
        },
        Plan::Limit { input, count } => Plan::Limit {
            input: Box::new(pass(*input)),
            count,
        },
        Plan::Expand {
            input,
            src,
            rel_var,
            rel_types,
            range,
            direction,
            dst,
            dst_label,
        } => Plan::Expand {
            input: Box::new(pass(*input)),
            src,
            rel_var,
            rel_types,
            range,
            direction,
            dst,
            dst_label,
        },
        Plan::Cartesian { left, right } => Plan::Cartesian {
            left: Box::new(pass(*left)),
            right: Box::new(pass(*right)),
        },
        Plan::Optional { input, optional } => Plan::Optional {
            input: Box::new(pass(*input)),
            optional: Box::new(pass(*optional)),
        },
        Plan::ShortestPath { input, all } => Plan::ShortestPath {
            input: Box::new(pass(*input)),
            all,
        },
        Plan::NamedPath { input, variable } => Plan::NamedPath {
            input: Box::new(pass(*input)),
            variable,
        },
        Plan::PlannerHint { input, hint } => Plan::PlannerHint {
            input: Box::new(pass(*input)),
            hint,
        },
        Plan::ProcedureCall {
            input,
            name,
            arguments,
            yields,
        } => Plan::ProcedureCall {
            input: Box::new(pass(*input)),
            name,
            arguments,
            yields,
        },
        Plan::LoadCsv {
            input,
            with_headers,
            url,
            variable,
            field_terminator,
        } => Plan::LoadCsv {
            input: Box::new(pass(*input)),
            with_headers,
            url,
            variable,
            field_terminator,
        },
        Plan::Create {
            input,
            unique,
            patterns,
        } => Plan::Create {
            input: Box::new(pass(*input)),
            unique,
            patterns,
        },
        Plan::Merge {
            input,
            pattern,
            actions,
        } => Plan::Merge {
            input: Box::new(pass(*input)),
            pattern,
            actions,
        },
        Plan::Set { input, items } => Plan::Set {
            input: Box::new(pass(*input)),
            items,
        },
        Plan::Remove { input, items } => Plan::Remove {
            input: Box::new(pass(*input)),
            items,
        },
        Plan::PeriodicCommit { input, limit } => Plan::PeriodicCommit {
            input: Box::new(pass(*input)),
            limit,
        },
        Plan::Explain { input } => Plan::Explain {
            input: Box::new(pass(*input)),
        },
        Plan::Profile { input } => Plan::Profile {
            input: Box::new(pass(*input)),
        },
        Plan::Distinct { input } => Plan::Distinct {
            input: Box::new(pass(*input)),
        },
        Plan::Union { left, right, all } => Plan::Union {
            left: Box::new(pass(*left)),
            right: Box::new(pass(*right)),
            all,
        },
        leaf @ (Plan::Empty | Plan::Scan { .. }) => leaf,
    }
}

fn apply_local(plan: Plan) -> Plan {
    match plan {
        Plan::Filter { input, pred } => try_push_filter(*input, pred),
        other => other,
    }
}

fn try_push_filter(input: Plan, pred: Expr) -> Plan {
    match input {
        Plan::Project {
            input: pi,
            include_existing,
            exprs,
        } => {
            // Push through Project unless the predicate references an alias
            // that the Project introduces.
            let aliases = project_aliases(&exprs);
            let used = used_vars(&pred);
            if used.is_disjoint(&aliases) {
                Plan::Project {
                    input: Box::new(try_push_filter(*pi, pred)),
                    include_existing,
                    exprs,
                }
            } else {
                Plan::Filter {
                    input: Box::new(Plan::Project {
                        input: pi,
                        include_existing,
                        exprs,
                    }),
                    pred,
                }
            }
        }
        Plan::Sort { input: si, keys } => Plan::Sort {
            input: Box::new(try_push_filter(*si, pred)),
            keys,
        },
        Plan::Cartesian { left, right } => {
            let used = used_vars(&pred);
            let left_vars = bound_vars(&left);
            let right_vars = bound_vars(&right);
            if !used.is_empty() && used.is_subset(&left_vars) {
                Plan::Cartesian {
                    left: Box::new(try_push_filter(*left, pred)),
                    right,
                }
            } else if !used.is_empty() && used.is_subset(&right_vars) {
                Plan::Cartesian {
                    left,
                    right: Box::new(try_push_filter(*right, pred)),
                }
            } else {
                Plan::Filter {
                    input: Box::new(Plan::Cartesian { left, right }),
                    pred,
                }
            }
        }
        Plan::Expand {
            input: ei,
            src,
            rel_var,
            rel_types,
            range,
            direction,
            dst,
            dst_label,
        } => {
            // Push below the Expand when pred doesn't reference the
            // vars Expand introduces (rel_var, dst). Those vars don't
            // exist in the input rows, so pushing is safe.
            let expand_introduces: HashSet<String> = [rel_var.as_deref(), dst.as_deref()]
                .into_iter()
                .flatten()
                .map(str::to_owned)
                .collect();
            if used_vars(&pred).is_disjoint(&expand_introduces) {
                Plan::Expand {
                    input: Box::new(try_push_filter(*ei, pred)),
                    src,
                    rel_var,
                    rel_types,
                    range,
                    direction,
                    dst,
                    dst_label,
                }
            } else {
                Plan::Filter {
                    input: Box::new(Plan::Expand {
                        input: ei,
                        src,
                        rel_var,
                        rel_types,
                        range,
                        direction,
                        dst,
                        dst_label,
                    }),
                    pred,
                }
            }
        }
        Plan::NamedPath { input, variable } => {
            if used_vars(&pred).contains(&variable) {
                Plan::Filter {
                    input: Box::new(Plan::NamedPath { input, variable }),
                    pred,
                }
            } else {
                Plan::NamedPath {
                    input: Box::new(try_push_filter(*input, pred)),
                    variable,
                }
            }
        }
        Plan::LoadCsv {
            input,
            with_headers,
            url,
            variable,
            field_terminator,
        } => {
            if used_vars(&pred).contains(&variable) {
                Plan::Filter {
                    input: Box::new(Plan::LoadCsv {
                        input,
                        with_headers,
                        url,
                        variable,
                        field_terminator,
                    }),
                    pred,
                }
            } else {
                Plan::LoadCsv {
                    input: Box::new(try_push_filter(*input, pred)),
                    with_headers,
                    url,
                    variable,
                    field_terminator,
                }
            }
        }
        Plan::PropertyMapFilter {
            input,
            variable,
            map,
        } => Plan::PropertyMapFilter {
            input: Box::new(try_push_filter(*input, pred)),
            variable,
            map,
        },
        // Don't push through Limit / Skip / Optional / leaves.
        other => Plan::Filter {
            input: Box::new(other),
            pred,
        },
    }
}

// --- helpers -------------------------------------------------------------

fn project_aliases(exprs: &[ProjectExpr]) -> HashSet<String> {
    exprs.iter().filter_map(|e| e.alias.clone()).collect()
}

fn used_vars(expr: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_expr(expr, &mut out);
    out
}

fn walk_expr(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Variable(v) => {
            out.insert(v.clone());
        }
        Expr::Property { base, .. } => walk_expr(base, out),
        Expr::LabelPredicate { expression, .. } => walk_expr(expression, out),
        Expr::Subscript { base, index } => {
            walk_expr(base, out);
            walk_expr(index, out);
        }
        Expr::Slice { base, start, end } => {
            walk_expr(base, out);
            if let Some(start) = start {
                walk_expr(start, out);
            }
            if let Some(end) = end {
                walk_expr(end, out);
            }
        }
        Expr::MapProjection { base, items } => {
            walk_expr(base, out);
            for item in items {
                match item {
                    MapProjectionItem::Literal { value, .. } => walk_expr(value, out),
                    MapProjectionItem::Variable(variable) => {
                        out.insert(variable.clone());
                    }
                    MapProjectionItem::Property(_) | MapProjectionItem::AllProperties => {}
                }
            }
        }
        Expr::FunctionCall { arguments, .. } => {
            if let FunctionArguments::Expressions(arguments) = arguments {
                for argument in arguments {
                    walk_expr(argument, out);
                }
            }
        }
        Expr::ListComprehension {
            variable,
            source,
            predicate,
            projection,
        } => {
            walk_expr(source, out);
            let mut local = HashSet::new();
            if let Some(predicate) = predicate {
                walk_expr(predicate, &mut local);
            }
            if let Some(projection) = projection {
                walk_expr(projection, &mut local);
            }
            local.remove(variable);
            out.extend(local);
        }
        Expr::CollectionPredicate {
            variable,
            source,
            predicate,
            ..
        } => {
            walk_expr(source, out);
            let mut local = HashSet::new();
            if let Some(predicate) = predicate {
                walk_expr(predicate, &mut local);
            }
            local.remove(variable);
            out.extend(local);
        }
        Expr::PatternComprehension {
            path_variable,
            pattern,
            predicate,
            projection,
        } => {
            let mut local = HashSet::new();
            walk_pattern_expressions(pattern, &mut local);
            if let Some(predicate) = predicate {
                walk_expr(predicate, &mut local);
            }
            walk_expr(projection, &mut local);
            if let Some(path_variable) = path_variable {
                local.remove(path_variable);
            }
            remove_pattern_bindings(pattern, &mut local);
            out.extend(local);
        }
        Expr::PatternExpression { pattern } => {
            walk_pattern_expressions(pattern, out);
            insert_pattern_bindings(pattern, out);
        }
        Expr::Filter {
            variable,
            source,
            predicate,
        } => {
            walk_expr(source, out);
            let mut local = HashSet::new();
            if let Some(predicate) = predicate {
                walk_expr(predicate, &mut local);
            }
            local.remove(variable);
            out.extend(local);
        }
        Expr::Extract {
            variable,
            source,
            projection,
        } => {
            walk_expr(source, out);
            let mut local = HashSet::new();
            if let Some(projection) = projection {
                walk_expr(projection, &mut local);
            }
            local.remove(variable);
            out.extend(local);
        }
        Expr::Reduce {
            accumulator,
            initial,
            variable,
            source,
            expression,
        } => {
            walk_expr(initial, out);
            walk_expr(source, out);
            let mut local = HashSet::new();
            if let Some(expression) = expression {
                walk_expr(expression, &mut local);
            }
            local.remove(accumulator);
            local.remove(variable);
            out.extend(local);
        }
        Expr::Case {
            operand,
            alternatives,
            else_expr,
        } => {
            if let Some(operand) = operand {
                walk_expr(operand, out);
            }
            for alternative in alternatives {
                walk_expr(&alternative.when, out);
                walk_expr(&alternative.then, out);
            }
            if let Some(else_expr) = else_expr {
                walk_expr(else_expr, out);
            }
        }
        Expr::ComparisonChain { arguments, .. } => {
            for argument in arguments {
                walk_expr(argument, out);
            }
        }
        Expr::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, out);
            walk_expr(rhs, out);
        }
        Expr::Unary { operand, .. } => walk_expr(operand, out),
        Expr::List(items) => {
            for i in items {
                walk_expr(i, out);
            }
        }
        Expr::Map(entries) => {
            for (_k, v) in entries {
                walk_expr(v, out);
            }
        }
        Expr::Literal(_) | Expr::Param(_) => {}
    }
}

fn walk_pattern_expressions(pattern: &Pattern, out: &mut HashSet<String>) {
    for (_, value) in &pattern.anchor.properties {
        walk_expr(value, out);
    }
    if let Some(property_map) = &pattern.anchor.property_map {
        walk_expr(property_map, out);
    }
    for chain in &pattern.chain {
        for (_, value) in &chain.rel.properties {
            walk_expr(value, out);
        }
        if let Some(property_map) = &chain.rel.property_map {
            walk_expr(property_map, out);
        }
        for (_, value) in &chain.node.properties {
            walk_expr(value, out);
        }
        if let Some(property_map) = &chain.node.property_map {
            walk_expr(property_map, out);
        }
    }
}

fn remove_pattern_bindings(pattern: &Pattern, out: &mut HashSet<String>) {
    if let Some(variable) = &pattern.path_variable {
        out.remove(variable);
    }
    if let Some(variable) = &pattern.anchor.var {
        out.remove(variable);
    }
    for chain in &pattern.chain {
        if let Some(variable) = &chain.rel.var {
            out.remove(variable);
        }
        if let Some(variable) = &chain.node.var {
            out.remove(variable);
        }
    }
}

fn insert_pattern_bindings(pattern: &Pattern, out: &mut HashSet<String>) {
    if let Some(variable) = &pattern.path_variable {
        out.insert(variable.clone());
    }
    if let Some(variable) = &pattern.anchor.var {
        out.insert(variable.clone());
    }
    for chain in &pattern.chain {
        if let Some(variable) = &chain.rel.var {
            out.insert(variable.clone());
        }
        if let Some(variable) = &chain.node.var {
            out.insert(variable.clone());
        }
    }
}

fn bound_vars(plan: &Plan) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_bound(plan, &mut out);
    out
}

fn walk_bound(plan: &Plan, out: &mut HashSet<String>) {
    match plan {
        Plan::Empty => {}
        Plan::Scan { var, .. } => {
            if let Some(v) = var {
                out.insert(v.clone());
            }
        }
        Plan::Expand {
            input,
            rel_var,
            dst,
            ..
        } => {
            walk_bound(input, out);
            if let Some(v) = rel_var {
                out.insert(v.clone());
            }
            if let Some(v) = dst {
                out.insert(v.clone());
            }
        }
        Plan::Filter { input, .. }
        | Plan::PropertyMapFilter { input, .. }
        | Plan::Sort { input, .. } => walk_bound(input, out),
        Plan::Project {
            input,
            include_existing,
            exprs,
        } => {
            if *include_existing {
                walk_bound(input, out);
            }
            for e in exprs {
                let name = e.alias.as_ref().or(match &e.expr {
                    Expr::Variable(variable) => Some(variable),
                    _ => None,
                });
                if let Some(name) = name {
                    out.insert(name.clone());
                }
            }
        }
        Plan::Skip { input, .. } | Plan::Limit { input, .. } => walk_bound(input, out),
        Plan::Cartesian { left, right } => {
            walk_bound(left, out);
            walk_bound(right, out);
        }
        Plan::Optional { input, optional } => {
            walk_bound(input, out);
            walk_bound(optional, out);
        }
        Plan::ShortestPath { input, .. } => walk_bound(input, out),
        Plan::NamedPath { input, variable } => {
            walk_bound(input, out);
            out.insert(variable.clone());
        }
        Plan::PlannerHint { input, .. } => walk_bound(input, out),
        Plan::ProcedureCall { input, yields, .. } => {
            walk_bound(input, out);
            out.extend(yields.iter().map(|item| item.binding().to_string()));
        }
        Plan::LoadCsv {
            input, variable, ..
        } => {
            walk_bound(input, out);
            out.insert(variable.clone());
        }
        Plan::Create {
            input, patterns, ..
        } => {
            walk_bound(input, out);
            for pattern in patterns {
                insert_pattern_bindings(pattern, out);
            }
        }
        Plan::Merge { input, pattern, .. } => {
            walk_bound(input, out);
            insert_pattern_bindings(pattern, out);
        }
        Plan::Set { input, .. } => walk_bound(input, out),
        Plan::Remove { input, .. } => walk_bound(input, out),
        Plan::PeriodicCommit { input, .. } => walk_bound(input, out),
        Plan::Explain { input } => walk_bound(input, out),
        Plan::Profile { input } => walk_bound(input, out),
        Plan::Distinct { input } => walk_bound(input, out),
        Plan::Union { left, right, .. } => {
            walk_bound(left, out);
            walk_bound(right, out);
        }
    }
}
