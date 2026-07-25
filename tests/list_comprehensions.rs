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
fn parses_list_comprehension_forms() {
    let minimal = first_return_expr("RETURN [x IN xs]");
    assert!(matches!(
        minimal,
        Expr::ListComprehension {
            variable,
            predicate: None,
            projection: None,
            ..
        } if variable == "x"
    ));

    let where_only = first_return_expr("RETURN [x IN xs WHERE x.active]");
    assert!(matches!(
        where_only,
        Expr::ListComprehension {
            predicate: Some(_),
            projection: None,
            ..
        }
    ));

    let projection_only = first_return_expr("RETURN [x IN xs | x.name]");
    assert!(matches!(
        projection_only,
        Expr::ListComprehension {
            predicate: None,
            projection: Some(_),
            ..
        }
    ));

    let full = first_return_expr("RETURN [x IN xs WHERE x.active | x.name]");
    assert!(matches!(
        full,
        Expr::ListComprehension {
            source,
            predicate: Some(_),
            projection: Some(projection),
            ..
        } if matches!(*source, Expr::Variable(ref name) if name == "xs")
            && matches!(*projection, Expr::Property { ref key, .. } if key == "name")
    ));
}

#[test]
fn parses_nested_comprehensions_with_shadowing() {
    let expr = first_return_expr("RETURN [x IN rows | [x IN x.children | x.name]]");
    assert!(matches!(
        expr,
        Expr::ListComprehension {
            variable,
            projection: Some(projection),
            ..
        } if variable == "x"
            && matches!(*projection, Expr::ListComprehension { ref variable, .. } if variable == "x")
    ));
}

#[test]
fn parses_all_collection_predicate_kinds_case_insensitively() {
    for (keyword, expected) in [
        ("ALL", CollectionPredicateKind::All),
        ("any", CollectionPredicateKind::Any),
        ("NoNe", CollectionPredicateKind::None),
        ("single", CollectionPredicateKind::Single),
    ] {
        let expr = first_return_expr(&format!("RETURN {keyword}(x IN xs WHERE x.enabled)"));
        assert!(matches!(
            expr,
            Expr::CollectionPredicate {
                kind,
                variable,
                predicate: Some(_),
                ..
            } if kind == expected && variable == "x"
        ));
    }

    let without_where = first_return_expr("RETURN all(x IN xs)");
    assert!(matches!(
        without_where,
        Expr::CollectionPredicate {
            kind: CollectionPredicateKind::All,
            predicate: None,
            ..
        }
    ));
}

#[test]
fn rejects_malformed_comprehensions_and_predicates() {
    for query in [
        "RETURN [IN xs]",
        "RETURN [x IN]",
        "RETURN [x IN xs WHERE]",
        "RETURN [x IN xs |]",
        "RETURN any(IN xs)",
        "RETURN single(x IN)",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_honors_local_scope_without_leaking_it() {
    let query =
        parse("MATCH (n) RETURN [x IN n.rows WHERE x.active AND n.enabled | x.name] AS names, x")
            .unwrap();
    let report = analyze(&query);
    let errors = report.errors().collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("`x`"));

    let query = parse("MATCH (n) RETURN all(x IN missing WHERE x.owner = outside)").unwrap();
    let messages = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message.contains("`missing`")));
    assert!(messages.iter().any(|message| message.contains("`outside`")));
    assert!(!messages.iter().any(|message| message.contains("`x`")));
}

#[test]
fn planner_and_pruner_preserve_only_external_dependencies() {
    let query =
        parse("MATCH (n), (m) RETURN [x IN n.items WHERE x.owner = m | x.name] AS names").unwrap();
    let plan = plan(&query).unwrap();
    assert!(matches!(
        plan,
        Plan::Project { ref exprs, .. }
            if matches!(exprs[0].expr, Expr::ListComprehension { .. })
    ));

    let demand = ["names".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&plan, &demand),
        ["n".to_string(), "m".to_string()].into()
    );
}

#[test]
fn optimizer_ignores_collection_predicate_iterator_dependencies() {
    let query = parse("MATCH (n), (m) WHERE all(x IN n.items WHERE x.active) RETURN n").unwrap();
    let optimized = optimize(plan(&query).unwrap());
    let Plan::Project { input, .. } = optimized else {
        panic!("expected Project");
    };
    let Plan::Cartesian { left, .. } = *input else {
        panic!("expected Cartesian below Project");
    };
    assert!(matches!(*left, Plan::Filter { .. }));
}
