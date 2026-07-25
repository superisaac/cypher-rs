use cypher_rs::*;

#[test]
fn parses_foreach_with_multiple_update_clauses() {
    let query = parse("FOREACH (x IN [1, 2, 3] | SET n.foo = x REMOVE n.bar)").unwrap();

    match &query.clauses[0] {
        Clause::Foreach(foreach) => {
            assert_eq!(foreach.variable, "x");
            assert!(matches!(&foreach.expression, Expr::List(items) if items.len() == 3));
            assert!(matches!(
                foreach.clauses.as_slice(),
                [Clause::Set(_), Clause::Remove(_)]
            ));
        }
        other => panic!("expected FOREACH, got {other:?}"),
    }
}

#[test]
fn parses_create_merge_and_delete_body_clauses() {
    let query =
        parse("FOREACH (x IN $items | CREATE (n {value: x}) MERGE (m {value: x}) DELETE m)")
            .unwrap();

    let Clause::Foreach(foreach) = &query.clauses[0] else {
        panic!("expected FOREACH");
    };
    assert!(matches!(
        foreach.clauses.as_slice(),
        [Clause::Create(_), Clause::Merge(_), Clause::Delete(_)]
    ));
}

#[test]
fn parses_nested_foreach_clauses() {
    let query =
        parse("MATCH (n) FOREACH (x IN [1] | FOREACH (y IN [x] | SET n.value = y)) RETURN n")
            .unwrap();

    let Clause::Foreach(outer) = &query.clauses[1] else {
        panic!("expected outer FOREACH");
    };
    let [Clause::Foreach(inner)] = outer.clauses.as_slice() else {
        panic!("expected nested FOREACH");
    };
    assert_eq!(inner.variable, "y");
    assert!(!analyze(&query).has_errors());
}

#[test]
fn foreach_keywords_are_case_insensitive_and_iterator_can_be_escaped() {
    let query = parse("foreach (`set` in [1] | set n.value = `set`)").unwrap();

    let Clause::Foreach(foreach) = &query.clauses[0] else {
        panic!("expected FOREACH");
    };
    assert_eq!(foreach.variable, "set");
    assert!(matches!(foreach.clauses.as_slice(), [Clause::Set(_)]));
}

#[test]
fn rejects_malformed_foreach_syntax() {
    assert!(parse("FOREACH x IN [1] | SET n.value = x").is_err());
    assert!(parse("FOREACH (x [1] | SET n.value = x)").is_err());
    assert!(parse("FOREACH (x IN [1] SET n.value = x)").is_err());
    assert!(parse("FOREACH (x IN [1] |)").is_err());
    assert!(parse("FOREACH (x IN [1] | SET n.value = x").is_err());
}

#[test]
fn foreach_iterator_is_local_to_its_body() {
    let query = parse("MATCH (n) FOREACH (x IN [1, 2] | SET n.value = x) RETURN x").unwrap();
    let report = analyze(&query);

    assert!(report
        .errors()
        .any(|issue| issue.code == "unbound-variable" && issue.message.contains("`x`")));
    assert!(!report.bindings.contains("x"));
}

#[test]
fn foreach_source_expression_uses_outer_scope() {
    let query = parse("MATCH (n) FOREACH (x IN missing | SET n.value = x) RETURN n").unwrap();
    let report = analyze(&query);

    assert!(report
        .errors()
        .any(|issue| issue.code == "unbound-variable" && issue.message.contains("`missing`")));
    assert!(!report
        .errors()
        .any(|issue| issue.code == "unbound-variable" && issue.message.contains("`x`")));
}

#[test]
fn planner_supports_foreach() {
    let query = parse("MATCH (n) FOREACH (x IN [1] | SET n.value = x) RETURN n").unwrap();
    let Plan::Project { input, .. } = plan(&query).unwrap() else {
        panic!("expected Project");
    };
    assert!(matches!(
        input.as_ref(),
        Plan::Foreach {
            variable,
            updates,
            ..
        } if variable == "x" && matches!(updates.as_ref(), Plan::Set { .. })
    ));
}
