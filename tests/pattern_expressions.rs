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
fn parses_bare_pattern_expression() {
    let expression = first_return_expr("RETURN (a)-[r:KNOWS]->(b)");
    match expression {
        Expr::PatternExpression { pattern } => {
            assert_eq!(pattern.anchor.var.as_deref(), Some("a"));
            assert_eq!(pattern.chain.len(), 1);
            assert_eq!(pattern.chain[0].rel.var.as_deref(), Some("r"));
            assert_eq!(pattern.chain[0].rel.types, ["KNOWS"]);
            assert_eq!(pattern.chain[0].node.var.as_deref(), Some("b"));
        }
        other => panic!("expected PatternExpression, got {other:?}"),
    }
}

#[test]
fn exists_accepts_pattern_expression_argument() {
    let query = parse("MATCH (n) RETURN EXISTS((n)-[:KNOWS]->())").unwrap();
    let Clause::Return(return_clause) = &query.clauses[1] else {
        panic!("expected RETURN");
    };
    assert!(matches!(
        &return_clause.items[0].expr,
        Expr::FunctionCall {
            name,
            arguments: FunctionArguments::Expressions(arguments),
            ..
        } if name.eq_ignore_ascii_case("exists")
            && matches!(arguments[0], Expr::PatternExpression { .. })
    ));
    assert!(!analyze(&query).has_errors());
}

#[test]
fn parses_shortest_path_pattern_expressions() {
    let expression = first_return_expr("RETURN shortestPath((a)-[*1..]->(b))");
    assert!(matches!(
        expression,
        Expr::PatternExpression { pattern }
            if pattern.shortest == Some(ShortestPathMode::Single)
                && pattern.chain[0].rel.range
                    == Some(RelationshipRange {
                        start: Some(1),
                        end: None,
                    })
    ));

    let expression = first_return_expr("RETURN allShortestPaths((a)-->(b))");
    assert!(matches!(
        expression,
        Expr::PatternExpression { pattern }
            if pattern.shortest == Some(ShortestPathMode::All)
    ));
}

#[test]
fn rejects_malformed_pattern_expressions() {
    for query in [
        "RETURN (a)-->",
        "RETURN --> (b)",
        "RETURN shortestPath((a)) trailing",
        "RETURN allShortestPaths()",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_requires_named_pattern_variables_to_be_bound() {
    let query = parse("MATCH (n) RETURN EXISTS((n)-[missing_rel]->(missing_node))").unwrap();
    let messages = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("`missing_rel`")));
    assert!(messages
        .iter()
        .any(|message| message.contains("`missing_node`")));
    assert!(!messages.iter().any(|message| message.contains("`n`")));

    let query = parse("MATCH (n), (m), ()-[r]-() RETURN (n)-[r]->(m)").unwrap();
    assert!(!analyze(&query).has_errors());
}

#[test]
fn semantic_analysis_checks_pattern_expression_schema() {
    struct EmptySchema;
    impl Schema for EmptySchema {
        fn has_label(&self, _label: &str) -> bool {
            false
        }

        fn has_rel_type(&self, _rel_type: &str) -> bool {
            false
        }
    }

    let query = parse("MATCH (n) RETURN (n:Missing)-[:BAD]->()").unwrap();
    let codes = analyze_with(&query, &EmptySchema)
        .errors()
        .map(|issue| issue.code)
        .collect::<Vec<_>>();
    assert!(codes.contains(&"unknown-label"));
    assert!(codes.contains(&"unknown-rel-type"));
}

#[test]
fn planner_optimizer_and_pruner_track_correlated_pattern_variables() {
    let query = parse("MATCH (n) RETURN EXISTS((n)-->() ) AS connected").unwrap();
    let logical_plan = plan(&query).unwrap();
    assert!(matches!(
        &logical_plan,
        Plan::Project { exprs, .. }
            if matches!(
                &exprs[0].expr,
                Expr::FunctionCall {
                    arguments: FunctionArguments::Expressions(arguments),
                    ..
                } if matches!(arguments[0], Expr::PatternExpression { .. })
            )
    ));
    let demand = ["connected".to_string()]
        .into_iter()
        .collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        ["n".to_string()].into()
    );

    let query = parse("MATCH (n), (m) WHERE EXISTS((n)-->() ) RETURN n").unwrap();
    let optimized = optimize(plan(&query).unwrap());
    let Plan::Project { input, .. } = optimized else {
        panic!("expected Project");
    };
    let Plan::Cartesian { left, .. } = *input else {
        panic!("expected Cartesian");
    };
    assert!(matches!(*left, Plan::Filter { .. }));
}
