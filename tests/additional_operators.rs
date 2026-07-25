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
fn xor_has_precedence_between_or_and_and() {
    let expression = return_expr("RETURN a OR b XOR c AND d");
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::Or, rhs, .. }
            if matches!(rhs.as_ref(), Expr::Binary { op: BinOp::Xor, rhs, .. }
                if matches!(rhs.as_ref(), Expr::Binary { op: BinOp::And, .. }))
    ));
}

#[test]
fn xor_is_case_insensitive_and_reserved() {
    assert!(matches!(
        return_expr("RETURN true xOr false"),
        Expr::Binary { op: BinOp::Xor, .. }
    ));
    assert!(parse("MATCH (xor) RETURN xor").is_err());
    assert!(parse("MATCH (xorValue) RETURN xorValue").is_ok());
}

#[test]
fn parses_regex_matching_with_reference_precedence() {
    let expression = return_expr("RETURN 1 + name =~ 'A.*'");
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::Add, rhs, .. }
            if matches!(rhs.as_ref(), Expr::Binary { op: BinOp::RegexMatch, .. })
    ));
}

#[test]
fn exponentiation_is_right_associative() {
    let expression = return_expr("RETURN 2 ^ 3 ^ 4");
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::Pow, rhs, .. }
            if matches!(rhs.as_ref(), Expr::Binary { op: BinOp::Pow, .. })
    ));
}

#[test]
fn unary_signs_bind_tighter_than_exponentiation() {
    let expression = return_expr("RETURN -2 ^ +3");
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::Pow, lhs, rhs }
            if matches!(lhs.as_ref(), Expr::Unary { op: UnOp::Neg, .. })
                && matches!(rhs.as_ref(), Expr::Unary { op: UnOp::Pos, .. })
    ));
}

#[test]
fn rejects_incomplete_operators() {
    for query in [
        "RETURN +",
        "RETURN 2 ^",
        "RETURN name =~",
        "RETURN XOR true",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_planner_optimizer_and_pruner_preserve_dependencies() {
    let query = parse(
        "MATCH (n), (m) WHERE n.name =~ m.pattern XOR n.enabled RETURN +(n.score ^ m.weight) AS rank",
    )
    .unwrap();
    assert!(!analyze(&query).has_errors());

    let logical_plan = plan(&query).unwrap();
    let optimized = optimize(logical_plan.clone());
    assert!(format!("{optimized}").contains("Filter"));

    let demand = ["rank".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        ["n".to_string(), "m".to_string()].into()
    );
}
