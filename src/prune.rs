//! Projection pruning analysis (v0.10).
//!
//! Two functions answer the column-tracking questions executors need
//! to materialize only what's referenced:
//!
//! - [`output_columns`] - what variables a plan's output rows
//!   contain. For `Project`, this is the set of aliases (or the
//!   underlying variable name when an item is a bare `Variable` with
//!   no alias). For other ops, it's the bindings the op produces or
//!   passes through.
//! - [`required_input_columns`] - given that the operators above this
//!   plan reference `outer_demand`, what variables must the plan's
//!   immediate input supply? Executors call this recursively to
//!   compute per-op input schemas without changing the plan algebra.
//!
//! No rewrites. No plan-tree changes. Pure analysis: pluggable
//! storage layers compute demand from the plan and decide what to
//! materialize.

use std::collections::HashSet;

use crate::ast::*;
use crate::plan::{Plan, ProjectExpr, SortKey};

/// Variables present in each row at the output of `plan`.
///
/// For `Project`, the columns are the aliases; if a `ProjectExpr`
/// has no alias and its expression is a bare `Variable(v)`, the
/// column is `v`. Anonymous projection items (e.g. `RETURN 1 + 2`
/// with no `AS`) contribute no column to this set.
///
/// For all other ops, the output schema is the same as the
/// underlying bindings the op exposes.
pub fn output_columns(plan: &Plan) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_output(plan, &mut out);
    out
}

fn walk_output(plan: &Plan, out: &mut HashSet<String>) {
    match plan {
        Plan::Empty
        | Plan::CreateIndex { .. }
        | Plan::DropIndex { .. }
        | Plan::CreateNodeConstraint { .. }
        | Plan::CreateRelationshipConstraint { .. }
        | Plan::DropNodeConstraint { .. }
        | Plan::DropRelationshipConstraint { .. } => {}
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
            walk_output(input, out);
            if let Some(v) = rel_var {
                out.insert(v.clone());
            }
            if let Some(v) = dst {
                out.insert(v.clone());
            }
        }
        Plan::ProcedureCall { input, yields, .. } => {
            walk_output(input, out);
            out.extend(yields.iter().map(|item| item.binding().to_string()));
        }
        Plan::LoadCsv {
            input, variable, ..
        } => {
            walk_output(input, out);
            out.insert(variable.clone());
        }
        Plan::Create {
            input, patterns, ..
        } => {
            walk_output(input, out);
            for pattern in patterns {
                insert_pattern_bindings(pattern, out);
            }
        }
        Plan::Merge { input, pattern, .. } => {
            walk_output(input, out);
            insert_pattern_bindings(pattern, out);
        }
        Plan::Set { input, .. } => walk_output(input, out),
        Plan::Remove { input, .. } => walk_output(input, out),
        Plan::Delete { input, .. } => walk_output(input, out),
        Plan::Unwind { input, alias, .. } => {
            walk_output(input, out);
            out.insert(alias.clone());
        }
        Plan::Foreach { input, .. } => walk_output(input, out),
        Plan::Start { input, points, .. } => {
            walk_output(input, out);
            out.extend(points.iter().map(|point| point.variable.clone()));
        }
        Plan::Filter { input, .. }
        | Plan::PropertyMapFilter { input, .. }
        | Plan::Sort { input, .. }
        | Plan::Skip { input, .. }
        | Plan::Limit { input, .. }
        | Plan::ShortestPath { input, .. }
        | Plan::PlannerHint { input, .. }
        | Plan::PeriodicCommit { input, .. }
        | Plan::Explain { input }
        | Plan::Profile { input } => walk_output(input, out),
        Plan::NamedPath { input, variable } => {
            walk_output(input, out);
            out.insert(variable.clone());
        }
        Plan::Project {
            input,
            include_existing,
            exprs,
        } => {
            // Projects replace the output schema. Collect each item's
            // visible name: alias if present, else the bare Variable
            // name, else nothing (anonymous).
            if *include_existing {
                walk_output(input, out);
            }
            for e in exprs {
                if let Some(name) = visible_name(e) {
                    out.insert(name);
                }
            }
        }
        Plan::Cartesian { left, right } => {
            walk_output(left, out);
            walk_output(right, out);
        }
        Plan::Optional { input, optional } => {
            walk_output(input, out);
            walk_output(optional, out);
        }
        Plan::Distinct { input } => walk_output(input, out),
        Plan::Union { left, .. } => walk_output(left, out),
    }
}

fn visible_name(e: &ProjectExpr) -> Option<String> {
    if let Some(a) = &e.alias {
        return Some(a.clone());
    }
    match &e.expr {
        Expr::Variable(v) => Some(v.clone()),
        _ => None,
    }
}

/// Variables that the **input** of `plan` must supply so the plan
/// can satisfy `outer_demand` (the columns operators above reference).
///
/// For leaf ops (`Empty`, `Scan`), the result is empty; there is no
/// input. For `Project`, the input must supply every variable
/// referenced by any project expression that contributes to a column
/// the outer scope demands; aliases introduced by the project are
/// stripped.
pub fn required_input_columns(plan: &Plan, outer_demand: &HashSet<String>) -> HashSet<String> {
    match plan {
        Plan::Empty
        | Plan::Scan { .. }
        | Plan::CreateIndex { .. }
        | Plan::DropIndex { .. }
        | Plan::CreateNodeConstraint { .. }
        | Plan::CreateRelationshipConstraint { .. }
        | Plan::DropNodeConstraint { .. }
        | Plan::DropRelationshipConstraint { .. } => HashSet::new(),
        Plan::Filter { pred, .. } => union(outer_demand, &used_vars_expr(pred)),
        Plan::PropertyMapFilter { variable, map, .. } => {
            let mut demand = union(outer_demand, &used_vars_expr(map));
            demand.insert(variable.clone());
            demand
        }
        Plan::Sort { keys, .. } => {
            let mut acc = outer_demand.clone();
            for k in keys {
                acc.extend(used_vars_expr(&k.expr));
            }
            acc
        }
        Plan::Skip { count, .. } | Plan::Limit { count, .. } => {
            // Skip / Limit don't reference row-bindings (count is
            // typically a literal or a parameter). Pass demand through.
            let mut acc = outer_demand.clone();
            acc.extend(used_vars_expr(count));
            acc
        }
        Plan::Project {
            input,
            include_existing,
            exprs,
        } => {
            // The input must supply every variable referenced by any
            // project item whose visible name is in outer_demand,
            // plus every variable referenced by anonymous items
            // (which are still evaluated even if not consumed by a
            // demand set).
            let explicit_names = exprs
                .iter()
                .filter_map(visible_name)
                .collect::<HashSet<_>>();
            let mut acc = if *include_existing {
                let demand = if outer_demand.is_empty() {
                    output_columns(input)
                } else {
                    outer_demand.clone()
                };
                demand
                    .difference(&explicit_names)
                    .cloned()
                    .collect::<HashSet<_>>()
            } else {
                HashSet::new()
            };
            for e in exprs {
                let name = visible_name(e);
                let referenced = match &name {
                    Some(n) => outer_demand.is_empty() || outer_demand.contains(n),
                    None => true,
                };
                if referenced {
                    acc.extend(used_vars_expr(&e.expr));
                }
            }
            acc
        }
        Plan::Expand {
            src, rel_var, dst, ..
        } => {
            // Expand produces rel_var and dst on top of its input.
            // The input must supply demand minus those, plus src.
            let mut acc: HashSet<String> = outer_demand
                .iter()
                .filter(|v| rel_var.as_ref() != Some(*v) && dst.as_ref() != Some(*v))
                .cloned()
                .collect();
            if let Some(s) = src {
                acc.insert(s.clone());
            }
            acc
        }
        Plan::ProcedureCall {
            arguments, yields, ..
        } => {
            let yielded = yields
                .iter()
                .map(|item| item.binding())
                .collect::<HashSet<_>>();
            let mut acc = outer_demand
                .iter()
                .filter(|name| !yielded.contains(name.as_str()))
                .cloned()
                .collect::<HashSet<_>>();
            for argument in arguments {
                acc.extend(used_vars_expr(argument));
            }
            acc
        }
        Plan::LoadCsv { url, variable, .. } => {
            let mut acc = outer_demand
                .iter()
                .filter(|name| *name != variable)
                .cloned()
                .collect::<HashSet<_>>();
            acc.extend(used_vars_expr(url));
            acc
        }
        Plan::Create { patterns, .. } => {
            let mut acc = outer_demand.clone();
            for pattern in patterns {
                remove_pattern_bindings(pattern, &mut acc);
                walk_pattern_expressions(pattern, &mut acc);
            }
            acc
        }
        Plan::Merge {
            pattern, actions, ..
        } => {
            let mut acc = outer_demand.clone();
            walk_pattern_expressions(pattern, &mut acc);
            for action in actions {
                for item in &action.items {
                    walk_set_item_expressions(item, &mut acc);
                }
            }
            // Pattern bindings are produced by MERGE and are available to its
            // actions; they do not need to be supplied by the input.
            remove_pattern_bindings(pattern, &mut acc);
            acc
        }
        Plan::Set { items, .. } => {
            let mut acc = outer_demand.clone();
            for item in items {
                walk_set_item_expressions(item, &mut acc);
            }
            acc
        }
        Plan::Remove { items, .. } => {
            let mut acc = outer_demand.clone();
            for item in items {
                walk_remove_item_expressions(item, &mut acc);
            }
            acc
        }
        Plan::Delete { expressions, .. } => {
            let mut acc = outer_demand.clone();
            for expression in expressions {
                walk_expr(expression, &mut acc);
            }
            acc
        }
        Plan::Unwind {
            expression, alias, ..
        } => {
            let mut acc = outer_demand
                .iter()
                .filter(|name| *name != alias)
                .cloned()
                .collect::<HashSet<_>>();
            walk_expr(expression, &mut acc);
            acc
        }
        Plan::Foreach {
            variable,
            expression,
            updates,
            ..
        } => {
            let mut acc = outer_demand.clone();
            walk_expr(expression, &mut acc);
            let mut produced = HashSet::new();
            walk_update_dependencies(updates, &mut acc, &mut produced);
            acc.remove(variable);
            for binding in produced {
                acc.remove(&binding);
            }
            acc
        }
        Plan::Start {
            points, predicate, ..
        } => {
            let bindings = points
                .iter()
                .map(|point| point.variable.as_str())
                .collect::<HashSet<_>>();
            let mut acc = outer_demand
                .iter()
                .filter(|name| !bindings.contains(name.as_str()))
                .cloned()
                .collect::<HashSet<_>>();
            for point in points {
                if let StartLookup::Index { value, .. } = &point.lookup {
                    walk_expr(value, &mut acc);
                }
            }
            if let Some(predicate) = predicate {
                walk_expr(predicate, &mut acc);
            }
            for binding in bindings {
                acc.remove(binding);
            }
            acc
        }
        Plan::Cartesian { .. } | Plan::Optional { .. } | Plan::Union { .. } => {
            // Both branches see the same outer demand, restricted to
            // variables that branch's subtree actually exposes.
            // For input-of-self purposes, the immediate input is the
            // root: callers typically recurse into `left` / `right` /
            // `optional` directly with split demand.
            outer_demand.clone()
        }
        Plan::Distinct { .. } => {
            // Distinct is transparent: it passes every column through unchanged.
            outer_demand.clone()
        }
        Plan::ShortestPath { .. } => outer_demand.clone(),
        Plan::PlannerHint { .. } => outer_demand.clone(),
        Plan::PeriodicCommit { .. } => outer_demand.clone(),
        Plan::Explain { .. } => outer_demand.clone(),
        Plan::Profile { .. } => outer_demand.clone(),
        Plan::NamedPath { variable, .. } => outer_demand
            .iter()
            .filter(|name| *name != variable)
            .cloned()
            .collect(),
    }
}

fn union(a: &HashSet<String>, b: &HashSet<String>) -> HashSet<String> {
    let mut out = a.clone();
    out.extend(b.iter().cloned());
    out
}

fn used_vars_expr(expr: &Expr) -> HashSet<String> {
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

fn walk_set_item_expressions(item: &SetItem, out: &mut HashSet<String>) {
    match item {
        SetItem::Property { property, value } => {
            walk_expr(property, out);
            walk_expr(value, out);
        }
        SetItem::AllProperties { variable, value }
        | SetItem::MergeProperties { variable, value } => {
            out.insert(variable.clone());
            walk_expr(value, out);
        }
        SetItem::Labels { variable, .. } => {
            out.insert(variable.clone());
        }
    }
}

fn walk_remove_item_expressions(item: &RemoveItem, out: &mut HashSet<String>) {
    match item {
        RemoveItem::Property(property) => walk_expr(property, out),
        RemoveItem::Labels { variable, .. } => {
            out.insert(variable.clone());
        }
    }
}

fn walk_update_dependencies(
    plan: &Plan,
    used: &mut HashSet<String>,
    produced: &mut HashSet<String>,
) {
    match plan {
        Plan::Empty => {}
        Plan::Create {
            input, patterns, ..
        } => {
            walk_update_dependencies(input, used, produced);
            for pattern in patterns {
                walk_pattern_expressions(pattern, used);
                insert_pattern_bindings(pattern, produced);
            }
        }
        Plan::Merge {
            input,
            pattern,
            actions,
        } => {
            walk_update_dependencies(input, used, produced);
            walk_pattern_expressions(pattern, used);
            for action in actions {
                for item in &action.items {
                    walk_set_item_expressions(item, used);
                }
            }
            insert_pattern_bindings(pattern, produced);
        }
        Plan::Set { input, items } => {
            walk_update_dependencies(input, used, produced);
            for item in items {
                walk_set_item_expressions(item, used);
            }
        }
        Plan::Remove { input, items } => {
            walk_update_dependencies(input, used, produced);
            for item in items {
                walk_remove_item_expressions(item, used);
            }
        }
        Plan::Delete {
            input, expressions, ..
        } => {
            walk_update_dependencies(input, used, produced);
            for expression in expressions {
                walk_expr(expression, used);
            }
        }
        Plan::Foreach {
            input,
            variable,
            expression,
            updates,
        } => {
            walk_update_dependencies(input, used, produced);
            walk_expr(expression, used);
            let mut nested_used = HashSet::new();
            let mut nested_produced = HashSet::new();
            walk_update_dependencies(updates, &mut nested_used, &mut nested_produced);
            nested_used.remove(variable);
            for binding in nested_produced {
                nested_used.remove(&binding);
            }
            used.extend(nested_used);
        }
        _ => {}
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

// SortKey is referenced from the doctest only; suppress the
// "unused import" warning when feature flags evolve.
#[allow(dead_code)]
fn _sort_key_anchor(_k: &SortKey) {}
