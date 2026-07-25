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
fn parses_simple_pattern_comprehension() {
    let expression = first_return_expr("RETURN [(a)-->(b) | b.name]");
    match expression {
        Expr::PatternComprehension {
            path_variable: None,
            pattern,
            predicate: None,
            projection,
        } => {
            assert_eq!(pattern.anchor.var.as_deref(), Some("a"));
            assert_eq!(pattern.chain.len(), 1);
            assert_eq!(pattern.chain[0].node.var.as_deref(), Some("b"));
            assert!(matches!(
                *projection,
                Expr::Property { ref key, .. } if key == "name"
            ));
        }
        other => panic!("expected PatternComprehension, got {other:?}"),
    }
}

#[test]
fn parses_named_filtered_variable_length_pattern_comprehension() {
    let expression = first_return_expr(
        "RETURN [p = (a:Person {id: outer.id})-[r:KNOWS*1..2]->(b) \
         WHERE b.active | {path: p, name: b.name}]",
    );
    match expression {
        Expr::PatternComprehension {
            path_variable: Some(path_variable),
            pattern,
            predicate: Some(_),
            projection,
        } => {
            assert_eq!(path_variable, "p");
            assert_eq!(pattern.anchor.labels, ["Person"]);
            assert_eq!(pattern.chain[0].rel.var.as_deref(), Some("r"));
            assert_eq!(pattern.chain[0].rel.types, ["KNOWS"]);
            assert_eq!(
                pattern.chain[0].rel.range,
                Some(RelationshipRange {
                    start: Some(1),
                    end: Some(2),
                })
            );
            assert!(matches!(*projection, Expr::Map(_)));
        }
        other => panic!("expected full PatternComprehension, got {other:?}"),
    }
}

#[test]
fn pattern_comprehensions_can_be_nested_and_use_escaped_names() {
    let expression =
        first_return_expr("RETURN [`path value` = (`start node`)-->(b) | [(b)-->(c) | c.name]]");
    assert!(matches!(
        expression,
        Expr::PatternComprehension {
            path_variable: Some(path_variable),
            projection,
            ..
        } if path_variable == "path value"
            && matches!(*projection, Expr::PatternComprehension { .. })
    ));
}

#[test]
fn rejects_malformed_pattern_comprehensions() {
    for query in [
        "RETURN [(a) | a]",
        "RETURN [(a)-->(b) |]",
        "RETURN [p = | p]",
        "RETURN [(a)-->(b) WHERE | b]",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_keeps_pattern_bindings_local() {
    let query =
        parse("MATCH (n) RETURN [p = (a)-[r]->(b) WHERE b.owner = n | [p, a, r, b]] AS paths, b")
            .unwrap();
    let errors = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("`b`"));

    let query = parse("RETURN [(a)-->(b) WHERE b.owner = missing | outside]").unwrap();
    let messages = analyze(&query)
        .errors()
        .map(|issue| issue.message.clone())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message.contains("`missing`")));
    assert!(messages.iter().any(|message| message.contains("`outside`")));
    assert!(!messages.iter().any(|message| message.contains("`a`")));
    assert!(!messages.iter().any(|message| message.contains("`b`")));
}

#[test]
fn semantic_analysis_checks_pattern_schema() {
    struct EmptySchema;
    impl Schema for EmptySchema {
        fn has_label(&self, _label: &str) -> bool {
            false
        }

        fn has_rel_type(&self, _rel_type: &str) -> bool {
            false
        }
    }

    let query = parse("RETURN [(a:Missing)-[:BAD]->(b) | b]").unwrap();
    let codes = analyze_with(&query, &EmptySchema)
        .errors()
        .map(|issue| issue.code)
        .collect::<Vec<_>>();
    assert!(codes.contains(&"unknown-label"));
    assert!(codes.contains(&"unknown-rel-type"));
}

#[test]
fn planner_optimizer_and_pruner_track_only_external_dependencies() {
    let query = parse(
        "MATCH (n) RETURN [p = (a {owner: n})-[r]->(b) \
         WHERE b.owner = n | {path: p, node: b}] AS paths",
    )
    .unwrap();
    let logical_plan = plan(&query).unwrap();
    assert!(matches!(
        &logical_plan,
        Plan::Project { exprs, .. }
            if matches!(exprs[0].expr, Expr::PatternComprehension { .. })
    ));
    let demand = ["paths".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        ["n".to_string()].into()
    );

    let query =
        parse("MATCH (n), (m) WHERE size([(a)-->(b) WHERE b.owner = n | b]) > 0 RETURN n").unwrap();
    let optimized = optimize(plan(&query).unwrap());
    let Plan::Project { input, .. } = optimized else {
        panic!("expected Project");
    };
    let Plan::Cartesian { left, .. } = *input else {
        panic!("expected Cartesian");
    };
    assert!(matches!(*left, Plan::Filter { .. }));
}
