use cypher_rs::*;

#[test]
fn parses_unique_node_constraint_reference_form() {
    let query = parse("DROP CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::DropNodeConstraint {
            variable,
            label,
            expression: Expr::Property { key, .. },
            unique: true,
        }) if variable == "f" && label == "Foo" && key == "bar"
    ));
}

#[test]
fn parses_node_property_existence_constraint() {
    let query = parse("DROP CONSTRAINT ON (f:Foo) ASSERT exists(f.bar)").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::DropNodeConstraint {
            expression: Expr::FunctionCall { name, .. },
            unique: false,
            ..
        }) if name == "exists"
    ));
}

#[test]
fn parses_relationship_property_constraints_in_each_direction() {
    for source in [
        "DROP CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since)",
        "DROP CONSTRAINT ON ()-[r:KNOWS]->() ASSERT exists(r.since)",
        "DROP CONSTRAINT ON ()<-[r:KNOWS]-() ASSERT exists(r.since)",
    ] {
        let query = parse(source).unwrap();
        assert!(matches!(
            &query.clauses[0],
            Clause::SchemaCommand(SchemaCommand::DropRelationshipConstraint {
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
        "drop constraint on (`node var`:`User Profile`) /* assertion */ \
         assert `node var`.`first name` is unique",
    )
    .unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::DropNodeConstraint {
            variable,
            label,
            expression: Expr::Property { key, .. },
            unique: true,
        }) if variable == "node var" && label == "User Profile" && key == "first name"
    ));
}

#[test]
fn rejects_malformed_drop_constraint_commands() {
    for source in [
        "DROP CONSTRAINT",
        "DROP CONSTRAINT ON",
        "DROP CONSTRAINT ON (:Foo) ASSERT exists(f.bar)",
        "DROP CONSTRAINT ON (f) ASSERT exists(f.bar)",
        "DROP CONSTRAINT ON (f:Foo)",
        "DROP CONSTRAINT ON (f:Foo) ASSERT",
        "DROP CONSTRAINT ON (f:Foo) ASSERT f.bar UNIQUE",
        "DROP CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since) IS UNIQUE",
        "DROP CONSTRAINT ON (a)-[r:KNOWS]-(b) ASSERT exists(r.since)",
        "DROP CONSTRAINT ON ()-[r:KNOWS|LIKES]-() ASSERT exists(r.since)",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn drop_constraint_is_an_isolated_schema_statement() {
    for source in [
        "DROP CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE RETURN 1",
        "MATCH (n) DROP CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE",
        "USING PERIODIC COMMIT DROP CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn create_and_drop_constraints_can_be_separate_statements() {
    let query = parse(
        "CREATE CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE; \
         DROP CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE; RETURN 1",
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
            SchemaCommand::DropNodeConstraint { .. }
        )]
    ));
}

#[test]
fn semantic_analysis_uses_the_constraint_target_scope() {
    let query = parse("DROP CONSTRAINT ON ()-[r:KNOWS]-() ASSERT r.since = missing.value").unwrap();
    let report = analyze(&query);
    let messages = report
        .errors()
        .map(|issue| issue.message.as_str())
        .collect::<Vec<_>>();

    assert!(messages.iter().any(|message| message.contains("`missing`")));
    assert!(!messages.iter().any(|message| message.contains("`r`")));
    assert!(report.bindings.is_empty());
}

#[test]
fn planner_reports_drop_constraint_as_unsupported() {
    for source in [
        "DROP CONSTRAINT ON (f:Foo) ASSERT f.bar IS UNIQUE",
        "DROP CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since)",
    ] {
        let query = parse(source).unwrap();
        assert_eq!(
            plan(&query),
            Err(PlanError::UnsupportedClause("DROP CONSTRAINT"))
        );
    }
}
