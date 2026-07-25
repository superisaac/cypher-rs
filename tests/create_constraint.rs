use cypher_rs::*;

#[test]
fn parses_unique_node_constraint_reference_form() {
    let query = parse("CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::CreateNodeConstraint {
            variable,
            label,
            expression: Expr::Property { key, .. },
            unique: true,
        }) if variable == "f" && label == "Foo" && key == "bar"
    ));
}

#[test]
fn parses_node_property_existence_constraint() {
    let query = parse("CREATE CONSTRAINT ON (f:Foo) ASSERT exists(f.bar)").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::CreateNodeConstraint {
            expression: Expr::FunctionCall { name, .. },
            unique: false,
            ..
        }) if name == "exists"
    ));
}

#[test]
fn parses_relationship_property_constraints_in_each_direction() {
    for source in [
        "CREATE CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since)",
        "CREATE CONSTRAINT ON ()-[r:KNOWS]->() ASSERT exists(r.since)",
        "CREATE CONSTRAINT ON ()<-[r:KNOWS]-() ASSERT exists(r.since)",
    ] {
        let query = parse(source).unwrap();
        assert!(matches!(
            &query.clauses[0],
            Clause::SchemaCommand(SchemaCommand::CreateRelationshipConstraint {
                variable,
                relationship_type,
                expression: Expr::FunctionCall { .. },
            }) if variable == "r" && relationship_type == "KNOWS"
        ));
    }
}

#[test]
fn supports_escaped_names_comments_and_case_insensitive_keywords() {
    let query = parse(
        "create constraint on (`node var`:`User Profile`) /* assertion */ \
         assert `node var`.`first name` is unique",
    )
    .unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::CreateNodeConstraint {
            variable,
            label,
            expression: Expr::Property { key, .. },
            unique: true,
        }) if variable == "node var" && label == "User Profile" && key == "first name"
    ));
}

#[test]
fn rejects_malformed_create_constraint_commands() {
    for source in [
        "CREATE CONSTRAINT",
        "CREATE CONSTRAINT ON",
        "CREATE CONSTRAINT ON (:Foo) ASSERT exists(f.bar)",
        "CREATE CONSTRAINT ON (f) ASSERT exists(f.bar)",
        "CREATE CONSTRAINT ON (f:Foo)",
        "CREATE CONSTRAINT ON (f:Foo) ASSERT",
        "CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar UNIQUE",
        "CREATE CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since) IS UNIQUE",
        "CREATE CONSTRAINT ON (a)-[r:KNOWS]-(b) ASSERT exists(r.since)",
        "CREATE CONSTRAINT ON ()-[r:KNOWS|LIKES]-() ASSERT exists(r.since)",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn create_constraint_is_an_isolated_schema_statement() {
    for source in [
        "CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE RETURN 1",
        "MATCH (n) CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE",
        "USING PERIODIC COMMIT CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn constraints_are_preserved_in_multiple_statements() {
    let query = parse(
        "CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE; \
         CREATE CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since); \
         RETURN 1",
    )
    .unwrap();
    let statements = query.statements().collect::<Vec<_>>();

    assert_eq!(query.statement_count(), 3);
    assert!(matches!(
        statements[0],
        [Clause::SchemaCommand(
            SchemaCommand::CreateNodeConstraint { .. }
        )]
    ));
    assert!(matches!(
        statements[1],
        [Clause::SchemaCommand(
            SchemaCommand::CreateRelationshipConstraint { .. }
        )]
    ));
}

#[test]
fn semantic_analysis_uses_the_constraint_target_scope() {
    let query = parse("CREATE CONSTRAINT ON (n:Person) ASSERT n.id = missing.id").unwrap();
    let report = analyze(&query);
    let messages = report
        .errors()
        .map(|issue| issue.message.as_str())
        .collect::<Vec<_>>();

    assert!(messages.iter().any(|message| message.contains("`missing`")));
    assert!(!messages.iter().any(|message| message.contains("`n`")));
    assert!(report.bindings.is_empty());
}

#[test]
fn planner_supports_create_constraint() {
    let query = parse("CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE").unwrap();
    assert!(matches!(
        plan(&query).unwrap(),
        Plan::CreateNodeConstraint { unique: true, .. }
    ));

    let query = parse("CREATE CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since)").unwrap();
    assert!(matches!(
        plan(&query).unwrap(),
        Plan::CreateRelationshipConstraint { .. }
    ));
}
