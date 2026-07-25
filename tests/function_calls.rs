use cypher_rs::*;
use std::collections::HashSet;

fn first_return_expr(query: &str) -> Expr {
    let query = parse(query).unwrap();
    match &query.clauses[0] {
        Clause::Return(return_clause) => return_clause.items[0].expr.clone(),
        clause => panic!("expected RETURN, got {clause:?}"),
    }
}

#[test]
fn parses_regular_and_nested_function_calls() {
    let expr = first_return_expr("RETURN coalesce(n.name, toString(42), 'unknown')");
    match expr {
        Expr::FunctionCall {
            name,
            distinct,
            arguments: FunctionArguments::Expressions(arguments),
        } => {
            assert_eq!(name, "coalesce");
            assert!(!distinct);
            assert_eq!(arguments.len(), 3);
            assert!(matches!(
                &arguments[1],
                Expr::FunctionCall { name, .. } if name == "toString"
            ));
        }
        other => panic!("expected function call, got {other:?}"),
    }
}

#[test]
fn parses_namespaced_and_zero_argument_function_calls() {
    let expr = first_return_expr("RETURN example.math.random()");
    assert!(matches!(
        expr,
        Expr::FunctionCall {
            name,
            distinct: false,
            arguments: FunctionArguments::Expressions(arguments),
        } if name == "example.math.random" && arguments.is_empty()
    ));
}

#[test]
fn parses_distinct_aggregate_argument() {
    let expr = first_return_expr("RETURN sum(DISTINCT n.amount)");
    assert!(matches!(
        expr,
        Expr::FunctionCall {
            name,
            distinct: true,
            arguments: FunctionArguments::Expressions(arguments),
        } if name == "sum" && arguments.len() == 1
    ));
}

#[test]
fn parses_count_wildcard() {
    let expr = first_return_expr("RETURN count(*)");
    assert_eq!(
        expr,
        Expr::FunctionCall {
            name: "count".into(),
            distinct: false,
            arguments: FunctionArguments::Wildcard,
        }
    );
}

#[test]
fn semantic_analysis_checks_function_arguments() {
    let query = parse("MATCH (n) RETURN coalesce(n.name, missing.name)").unwrap();
    let report = analyze(&query);
    assert!(report
        .errors()
        .any(|issue| issue.code == "unbound-variable" && issue.message.contains("`missing`")));
}

#[test]
fn planner_preserves_aggregate_function_expression() {
    let query = parse("MATCH (n) RETURN count(*) AS total").unwrap();
    let plan = plan(&query).unwrap();
    match plan {
        Plan::Project { exprs, .. } => {
            assert_eq!(exprs[0].alias.as_deref(), Some("total"));
            assert!(matches!(
                exprs[0].expr,
                Expr::FunctionCall {
                    ref name,
                    arguments: FunctionArguments::Wildcard,
                    ..
                } if name == "count"
            ));
        }
        other => panic!("expected Project, got {other:?}"),
    }
}

#[test]
fn optimizer_and_pruner_see_variables_in_function_arguments() {
    let query = parse("MATCH (n) WHERE coalesce(n.active, false) RETURN n").unwrap();
    let optimized = optimize(plan(&query).unwrap());
    assert!(matches!(optimized, Plan::Project { .. }));

    let query = parse("MATCH (n) RETURN coalesce(n.name, 'unknown') AS name").unwrap();
    let plan = plan(&query).unwrap();
    let outer_demand = ["name".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&plan, &outer_demand),
        ["n".to_string()].into()
    );
}
