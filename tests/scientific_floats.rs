use cypher_rs::*;

fn return_exprs(query: &str) -> Vec<Expr> {
    let query = parse(query).unwrap();
    let Clause::Return(return_clause) = &query.clauses[0] else {
        panic!("expected RETURN");
    };
    return_clause
        .items
        .iter()
        .map(|item| item.expr.clone())
        .collect()
}

#[test]
fn parses_integer_mantissa_scientific_literals() {
    let expressions = return_exprs("RETURN 1e3, 2E4, 3e+2, 4E-1");
    assert_eq!(
        expressions,
        [1000.0, 20000.0, 300.0, 0.4]
            .into_iter()
            .map(|value| Expr::Literal(Literal::Float(value)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn parses_decimal_mantissa_scientific_literals() {
    let expressions = return_exprs("RETURN 1.5e2, .5E+2, 1.e2");
    assert_eq!(
        expressions,
        [150.0, 50.0, 100.0]
            .into_iter()
            .map(|value| Expr::Literal(Literal::Float(value)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn also_accepts_leading_dot_decimal_literals() {
    assert_eq!(
        return_exprs("RETURN .5")[0],
        Expr::Literal(Literal::Float(0.5))
    );
}

#[test]
fn signs_remain_unary_expressions() {
    let expressions = return_exprs("RETURN -1e3, +2.5e-2");
    assert!(matches!(
        &expressions[0],
        Expr::Unary { op: UnOp::Neg, operand }
            if **operand == Expr::Literal(Literal::Float(1000.0))
    ));
    assert!(matches!(
        &expressions[1],
        Expr::Unary { op: UnOp::Pos, operand }
            if **operand == Expr::Literal(Literal::Float(0.025))
    ));
}

#[test]
fn scientific_literals_participate_in_expression_precedence() {
    let expression = return_exprs("RETURN 1e2 + 2e1 * 3").remove(0);
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::Add, rhs, .. }
            if matches!(rhs.as_ref(), Expr::Binary { op: BinOp::Mul, .. })
    ));
}

#[test]
fn rejects_malformed_and_non_finite_scientific_literals() {
    for query in [
        "RETURN 1e",
        "RETURN 1e+",
        "RETURN .e2",
        "RETURN 1.2e-",
        "RETURN 1e309",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn planner_and_cost_model_preserve_scientific_literals() {
    let logical_plan = plan(&parse("RETURN 1e3 AS value LIMIT 2.5e1").unwrap()).unwrap();
    let Plan::Limit { input, count } = &logical_plan else {
        panic!("expected Limit");
    };
    assert_eq!(*count, Expr::Literal(Literal::Float(25.0)));
    assert!(matches!(input.as_ref(), Plan::Project { exprs, .. }
        if exprs[0].expr == Expr::Literal(Literal::Float(1000.0))));
    assert_eq!(
        estimate(&logical_plan, &CardinalityCostModel::default()).cardinality,
        1.0
    );
}
