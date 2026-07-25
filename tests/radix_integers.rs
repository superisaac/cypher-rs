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
fn parses_hexadecimal_integer_literals() {
    assert_eq!(
        return_exprs("RETURN 0x0, 0x2a, 0xCAFE"),
        [0, 42, 51966]
            .into_iter()
            .map(|value| Expr::Literal(Literal::Int(value)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn parses_legacy_octal_integer_literals() {
    assert_eq!(
        return_exprs("RETURN 00, 077, 01234567"),
        [0, 63, 342391]
            .into_iter()
            .map(|value| Expr::Literal(Literal::Int(value)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn signs_remain_unary_expressions() {
    let expressions = return_exprs("RETURN -0x2a, +077");
    assert!(matches!(
        &expressions[0],
        Expr::Unary { op: UnOp::Neg, operand }
            if **operand == Expr::Literal(Literal::Int(42))
    ));
    assert!(matches!(
        &expressions[1],
        Expr::Unary { op: UnOp::Pos, operand }
            if **operand == Expr::Literal(Literal::Int(63))
    ));
}

#[test]
fn radix_literals_participate_in_arithmetic() {
    let expression = return_exprs("RETURN 0x10 + 010 * 2").remove(0);
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::Add, rhs, .. }
            if matches!(rhs.as_ref(), Expr::Binary { op: BinOp::Mul, .. })
    ));
}

#[test]
fn relationship_ranges_accept_radix_integer_bounds() {
    let query = parse("MATCH (a)-[*0x2..010]->(b) RETURN a").unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    assert_eq!(
        match_clause.patterns[0].chain[0].rel.range,
        Some(RelationshipRange {
            start: Some(2),
            end: Some(8),
        })
    );
}

#[test]
fn rejects_invalid_and_overflowing_radix_literals() {
    for query in [
        "RETURN 0x",
        "RETURN 0xG",
        "RETURN 08",
        "RETURN 078",
        "RETURN 0o77",
        "RETURN 0x8000000000000000",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn planner_and_cost_model_preserve_radix_values() {
    let logical_plan = plan(&parse("RETURN 0x20 AS value LIMIT 040").unwrap()).unwrap();
    let Plan::Limit { input, count } = &logical_plan else {
        panic!("expected Limit");
    };
    assert_eq!(*count, Expr::Literal(Literal::Int(32)));
    assert!(matches!(input.as_ref(), Plan::Project { exprs, .. }
        if exprs[0].expr == Expr::Literal(Literal::Int(32))));
    assert_eq!(
        estimate(&logical_plan, &CardinalityCostModel::default()).cardinality,
        1.0
    );
}
