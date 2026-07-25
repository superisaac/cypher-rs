use std::collections::HashSet;

use cypher_rs::*;

fn return_exprs(query: &str) -> Vec<Expr> {
    let query = parse(query).unwrap();
    match &query.clauses[0] {
        Clause::Return(return_clause) => return_clause
            .items
            .iter()
            .map(|item| item.expr.clone())
            .collect(),
        clause => panic!("expected RETURN, got {clause:?}"),
    }
}

#[test]
fn parses_dynamic_and_chained_subscripts() {
    let expressions = return_exprs("RETURN foo[n], [[1, 2, 3]][0], matrix[0][1].value");
    assert!(matches!(
        &expressions[0],
        Expr::Subscript { base, index }
            if matches!(base.as_ref(), Expr::Variable(name) if name == "foo")
                && matches!(index.as_ref(), Expr::Variable(name) if name == "n")
    ));
    assert!(matches!(
        &expressions[1],
        Expr::Subscript { base, .. } if matches!(base.as_ref(), Expr::List(_))
    ));
    assert!(matches!(
        &expressions[2],
        Expr::Property { base, key }
            if key == "value"
                && matches!(base.as_ref(), Expr::Subscript { base, .. }
                    if matches!(base.as_ref(), Expr::Subscript { .. }))
    ));
}

#[test]
fn parses_all_slice_bound_forms() {
    let expressions = return_exprs("RETURN xs[1..5], xs[..n + 5], xs[1..], xs[..]");
    assert!(matches!(
        &expressions[0],
        Expr::Slice {
            start: Some(_),
            end: Some(_),
            ..
        }
    ));
    assert!(matches!(
        &expressions[1],
        Expr::Slice {
            start: None,
            end: Some(end),
            ..
        } if matches!(end.as_ref(), Expr::Binary { op: BinOp::Add, .. })
    ));
    assert!(matches!(
        &expressions[2],
        Expr::Slice {
            start: Some(_),
            end: None,
            ..
        }
    ));
    assert!(matches!(
        &expressions[3],
        Expr::Slice {
            start: None,
            end: None,
            ..
        }
    ));
}

#[test]
fn subscript_binds_tighter_than_in() {
    let expression = return_exprs("RETURN 3 IN [[1, 2, 3]][0]").remove(0);
    assert!(matches!(
        expression,
        Expr::Binary {
            op: BinOp::In,
            rhs,
            ..
        } if matches!(*rhs, Expr::Subscript { .. })
    ));
}

#[test]
fn parses_map_projection_selectors_and_postfix_chains() {
    let expressions = return_exprs(
        "RETURN n {}, n { answer: 42, .name, other, .* }, n { .profile }.profile.tags[0]",
    );
    assert!(matches!(
        &expressions[0],
        Expr::MapProjection { items, .. } if items.is_empty()
    ));
    match &expressions[1] {
        Expr::MapProjection { base, items } => {
            assert!(matches!(base.as_ref(), Expr::Variable(name) if name == "n"));
            assert!(matches!(
                &items[0],
                MapProjectionItem::Literal { key, value }
                    if key == "answer" && matches!(value, Expr::Literal(Literal::Int(42)))
            ));
            assert_eq!(items[1], MapProjectionItem::Property("name".into()));
            assert_eq!(items[2], MapProjectionItem::Variable("other".into()));
            assert_eq!(items[3], MapProjectionItem::AllProperties);
        }
        other => panic!("expected MapProjection, got {other:?}"),
    }
    assert!(matches!(&expressions[2], Expr::Subscript { .. }));
}

#[test]
fn rejects_malformed_postfix_expressions() {
    for query in [
        "RETURN xs[]",
        "RETURN xs[1..2..3]",
        "RETURN xs[1",
        "RETURN n { .name, }",
        "RETURN n { key: }",
        "RETURN n { .* other }",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_checks_every_external_dependency() {
    let query = parse(
        "MATCH (n) RETURN n.items[missing], n.values[start..finish], \
         n { .name, copied: outside.value, included }",
    )
    .unwrap();
    let messages = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    for variable in ["missing", "start", "finish", "outside", "included"] {
        assert!(messages.iter().any(|message| message.contains(variable)));
    }
    assert!(!messages.iter().any(|message| message.contains("`n`")));
}

#[test]
fn planner_optimizer_and_pruner_preserve_postfix_dependencies() {
    let query = parse("MATCH (n), (m) RETURN n { first: n.items[m.index], .name, m } AS projected")
        .unwrap();
    let logical_plan = plan(&query).unwrap();
    assert!(matches!(
        &logical_plan,
        Plan::Project { exprs, .. }
            if matches!(exprs[0].expr, Expr::MapProjection { .. })
    ));
    let demand = ["projected".to_string()]
        .into_iter()
        .collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        ["n".to_string(), "m".to_string()].into()
    );

    let query = parse("MATCH (n), (m) WHERE n.items[0] = 1 RETURN n").unwrap();
    let optimized = optimize(plan(&query).unwrap());
    let Plan::Project { input, .. } = optimized else {
        panic!("expected Project");
    };
    let Plan::Cartesian { left, .. } = *input else {
        panic!("expected Cartesian");
    };
    assert!(matches!(*left, Plan::Filter { .. }));

    let query = parse("MATCH (n), (m) WHERE n.items[m.index] = 1 RETURN n").unwrap();
    let optimized = optimize(plan(&query).unwrap());
    assert!(matches!(
        optimized,
        Plan::Project { input, .. } if matches!(*input, Plan::Filter { .. })
    ));
}
