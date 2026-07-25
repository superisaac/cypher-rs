use std::collections::HashSet;

use cypher_rs::*;

fn first_return_expr(query: &str) -> Expr {
    let query = parse(query).unwrap();
    match &query.clauses[0] {
        Clause::Return(return_clause) => return_clause.items[0].expr.clone(),
        clause => panic!("expected RETURN, got {clause:?}"),
    }
}

#[test]
fn parses_filter_with_and_without_where() {
    let expression = first_return_expr("RETURN FILTER(x IN xs WHERE x.active)");
    assert!(matches!(
        expression,
        Expr::Filter {
            variable,
            predicate: Some(_),
            ..
        } if variable == "x"
    ));

    let expression = first_return_expr("RETURN filter(x IN xs)");
    assert!(matches!(
        expression,
        Expr::Filter {
            predicate: None,
            ..
        }
    ));
}

#[test]
fn parses_extract_with_and_without_projection() {
    let expression = first_return_expr("RETURN EXTRACT(x IN xs | x.name)");
    assert!(matches!(
        expression,
        Expr::Extract {
            variable,
            projection: Some(_),
            ..
        } if variable == "x"
    ));

    let expression = first_return_expr("RETURN extract(x IN xs)");
    assert!(matches!(
        expression,
        Expr::Extract {
            projection: None,
            ..
        }
    ));
}

#[test]
fn parses_reduce_with_and_without_expression() {
    let expression = first_return_expr("RETURN REDUCE(total = 0, x IN xs | total + x)");
    assert!(matches!(
        expression,
        Expr::Reduce {
            accumulator,
            variable,
            initial,
            expression: Some(_),
            ..
        } if accumulator == "total"
            && variable == "x"
            && matches!(*initial, Expr::Literal(Literal::Int(0)))
    ));

    let expression = first_return_expr("RETURN reduce(total = 0, x IN xs)");
    assert!(matches!(
        expression,
        Expr::Reduce {
            expression: None,
            ..
        }
    ));
}

#[test]
fn supports_nested_expressions_and_escaped_local_names() {
    let expression = first_return_expr(
        "RETURN EXTRACT(`item value` IN FILTER(x IN rows WHERE x.active) | \
         REDUCE(`total value` = 0, y IN `item value`.values | `total value` + y))",
    );
    assert!(matches!(
        expression,
        Expr::Extract {
            variable,
            source,
            projection: Some(projection),
        } if variable == "item value"
            && matches!(*source, Expr::Filter { .. })
            && matches!(*projection, Expr::Reduce { ref accumulator, .. }
                if accumulator == "total value")
    ));
}

#[test]
fn rejects_malformed_collection_functions() {
    for query in [
        "RETURN FILTER(IN xs)",
        "RETURN FILTER(x IN)",
        "RETURN FILTER(x IN xs WHERE)",
        "RETURN EXTRACT(x xs)",
        "RETURN EXTRACT(x IN xs |)",
        "RETURN REDUCE(total, x IN xs | total + x)",
        "RETURN REDUCE(total = 0 x IN xs | total + x)",
        "RETURN REDUCE(total = 0, x IN | total + x)",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_honors_collection_function_scopes() {
    let query =
        parse("MATCH (n) RETURN FILTER(x IN n.items WHERE x.owner = n) AS filtered, x").unwrap();
    let errors = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("`x`"));

    let query =
        parse("RETURN REDUCE(acc = missing_init, item IN missing_source | acc + item + outside)")
            .unwrap();
    let messages = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    for variable in ["missing_init", "missing_source", "outside"] {
        assert!(messages.iter().any(|message| message.contains(variable)));
    }
    assert!(!messages.iter().any(|message| message.contains("`acc`")));
    assert!(!messages.iter().any(|message| message.contains("`item`")));
}

#[test]
fn reduce_locals_are_not_visible_in_initial_or_source() {
    let query = parse("RETURN REDUCE(acc = acc, item IN item.values | acc + item)").unwrap();
    let messages = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().any(|message| message.contains("`acc`")));
    assert!(messages.iter().any(|message| message.contains("`item`")));
}

#[test]
fn planner_optimizer_and_pruner_track_only_external_dependencies() {
    let query = parse(
        "MATCH (n), (m) RETURN REDUCE(total = m.seed, x IN n.values | \
         total + x + m.offset) AS result",
    )
    .unwrap();
    let logical_plan = plan(&query).unwrap();
    assert!(matches!(
        &logical_plan,
        Plan::Project { exprs, .. } if matches!(exprs[0].expr, Expr::Reduce { .. })
    ));
    let demand = ["result".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        ["n".to_string(), "m".to_string()].into()
    );

    let query =
        parse("MATCH (n), (m) WHERE size(FILTER(x IN n.items WHERE x.active)) > 0 RETURN n")
            .unwrap();
    let optimized = optimize(plan(&query).unwrap());
    let Plan::Project { input, .. } = optimized else {
        panic!("expected Project");
    };
    let Plan::Cartesian { left, .. } = *input else {
        panic!("expected Cartesian");
    };
    assert!(matches!(*left, Plan::Filter { .. }));
}
