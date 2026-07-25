use std::collections::HashSet;

use cypher_rs::*;

fn return_expr(query: &str) -> Expr {
    let query = parse(query).unwrap();
    let Clause::Return(return_clause) = &query.clauses[0] else {
        panic!("expected RETURN");
    };
    return_clause.items[0].expr.clone()
}

#[test]
fn parses_same_operator_comparison_chains() {
    let expression = return_expr("RETURN 4 > value > 2");
    assert!(matches!(
        expression,
        Expr::ComparisonChain { operators, arguments }
            if operators == [BinOp::Gt, BinOp::Gt] && arguments.len() == 3
    ));
}

#[test]
fn parses_mixed_comparison_chains() {
    let expression = return_expr("RETURN 2 <= value >= 5");
    assert!(matches!(
        expression,
        Expr::ComparisonChain { operators, arguments }
            if operators == [BinOp::Lte, BinOp::Gte] && arguments.len() == 3
    ));
}

#[test]
fn comparison_arguments_preserve_arithmetic_precedence() {
    let expression = return_expr("RETURN low + 1 < value * 2 <= high ^ 3");
    let Expr::ComparisonChain {
        operators,
        arguments,
    } = expression
    else {
        panic!("expected ComparisonChain");
    };
    assert_eq!(operators, [BinOp::Lt, BinOp::Lte]);
    assert!(matches!(arguments[0], Expr::Binary { op: BinOp::Add, .. }));
    assert!(matches!(arguments[1], Expr::Binary { op: BinOp::Mul, .. }));
    assert!(matches!(arguments[2], Expr::Binary { op: BinOp::Pow, .. }));
}

#[test]
fn single_comparisons_remain_binary_expressions() {
    assert!(matches!(
        return_expr("RETURN a < b"),
        Expr::Binary { op: BinOp::Lt, .. }
    ));
}

#[test]
fn equality_operators_cannot_form_comparison_chains() {
    for query in ["RETURN a = b = c", "RETURN a <> b <> c", "RETURN a <"] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_checks_every_argument() {
    let query = parse("MATCH (middle) RETURN missing_low < middle.value < missing_high").unwrap();
    let messages = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("`missing_low`")));
    assert!(messages
        .iter()
        .any(|message| message.contains("`missing_high`")));
}

#[test]
fn planner_optimizer_and_pruner_preserve_chain_dependencies() {
    let query = parse(
        "MATCH (low), (middle), (high) WHERE low.value < middle.value <= high.value RETURN low.value < middle.value < high.value AS ordered",
    )
    .unwrap();
    assert!(!analyze(&query).has_errors());

    let logical_plan = plan(&query).unwrap();
    let optimized = optimize(logical_plan.clone());
    assert!(format!("{optimized}").contains("Filter"));

    let demand = ["ordered".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        ["low".to_string(), "middle".to_string(), "high".to_string()].into()
    );
}
