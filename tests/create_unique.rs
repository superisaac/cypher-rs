use cypher_rs::{analyze, parse, plan, Clause, Plan};

#[test]
fn parses_reference_create_unique_form() {
    let query = parse("CREATE UNIQUE (n)-[:KNOWS]->(f)").unwrap();
    let Clause::Create(create) = &query.clauses[0] else {
        panic!("expected CREATE");
    };
    assert!(create.unique);
    assert_eq!(create.patterns.len(), 1);
    assert_eq!(create.patterns[0].chain.len(), 1);
}

#[test]
fn regular_create_remains_non_unique() {
    let query = parse("CREATE (n)").unwrap();
    let Clause::Create(create) = &query.clauses[0] else {
        panic!("expected CREATE");
    };
    assert!(!create.unique);
}

#[test]
fn supports_multiple_patterns_properties_and_escaped_names() {
    let query =
        parse("create unique (`from` {id: $id})-[:`owns type`]->(item), (item)-[:TAGGED]->(tag)")
            .unwrap();
    let Clause::Create(create) = &query.clauses[0] else {
        panic!("expected CREATE UNIQUE");
    };
    assert!(create.unique);
    assert_eq!(create.patterns.len(), 2);
    assert!(!analyze(&query).has_errors());
}

#[test]
fn works_inside_foreach_update_bodies() {
    let query = parse("FOREACH (x IN $items | CREATE UNIQUE (n {id: x.id}))").unwrap();
    let Clause::Foreach(foreach) = &query.clauses[0] else {
        panic!("expected FOREACH");
    };
    assert!(matches!(
        &foreach.clauses[0],
        Clause::Create(create) if create.unique
    ));
}

#[test]
fn planner_preserves_create_unique() {
    let query = parse("CREATE UNIQUE (n)").unwrap();
    assert!(matches!(
        plan(&query).unwrap(),
        Plan::Create {
            unique: true,
            patterns,
            ..
        } if patterns.len() == 1
    ));
}

#[test]
fn rejects_malformed_create_unique() {
    for source in [
        "CREATE UNIQUE",
        "CREATE UNIQUE UNIQUE (n)",
        "CREATE UNIQUE ()-[]-",
        "CREATEUNIQUE (n)",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}
