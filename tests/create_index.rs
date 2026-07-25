use cypher_rs::*;

#[test]
fn parses_create_index_reference_form() {
    let query = parse("CREATE INDEX ON :Foo(bar)").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::CreateIndex { label, properties })
            if label == "Foo" && properties == &["bar"]
    ));
    assert_eq!(query.clause_count(), 1);
}

#[test]
fn parses_composite_indexes_and_escaped_names() {
    let query = parse("CREATE INDEX ON :`User Profile`(`first name`, `index`)").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::CreateIndex { label, properties })
            if label == "User Profile" && properties == &["first name", "index"]
    ));
}

#[test]
fn keywords_are_case_insensitive_and_comments_are_supported() {
    let query = parse("create /* kind */ index on /* label */ :Person(name, age)").unwrap();
    assert!(matches!(
        query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::CreateIndex { .. })
    ));

    assert!(parse("CREATE INDEXED ON :Person(name)").is_err());
    assert!(parse("CREATE INDEX ONLY :Person(name)").is_err());
}

#[test]
fn rejects_malformed_create_index_commands() {
    for source in [
        "CREATE INDEX",
        "CREATE INDEX ON",
        "CREATE INDEX ON Person(name)",
        "CREATE INDEX ON :(name)",
        "CREATE INDEX ON :Person",
        "CREATE INDEX ON :Person()",
        "CREATE INDEX ON :Person(name,)",
        "CREATE INDEX ON :Person(name",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn schema_commands_cannot_mix_with_query_clauses_or_periodic_commit() {
    for source in [
        "CREATE INDEX ON :Person(name) RETURN 1",
        "MATCH (n) CREATE INDEX ON :Person(name)",
        "USING PERIODIC COMMIT CREATE INDEX ON :Person(name)",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }

    assert!(matches!(
        parse("CREATE (n)").unwrap().clauses[0],
        Clause::Create(_)
    ));
}

#[test]
fn preserves_schema_commands_as_independent_statements() {
    let query =
        parse("CREATE INDEX ON :Person(name); RETURN 1; CREATE INDEX ON :Movie(title);").unwrap();
    let statements = query.statements().collect::<Vec<_>>();

    assert_eq!(query.statement_count(), 3);
    assert_eq!(query.clause_count(), 3);
    assert!(matches!(statements[0], [Clause::SchemaCommand(_)]));
    assert!(matches!(statements[1], [Clause::Return(_)]));
    assert!(matches!(statements[2], [Clause::SchemaCommand(_)]));
}

#[test]
fn semantic_analysis_accepts_schema_commands_without_query_bindings() {
    let query = parse("CREATE INDEX ON :Person(name, email)").unwrap();
    let report = analyze(&query);

    assert!(!report.has_errors());
    assert!(report.bindings.is_empty());
}

#[test]
fn planner_reports_create_index_as_unsupported() {
    let query = parse("CREATE INDEX ON :Person(name)").unwrap();
    assert_eq!(
        plan(&query),
        Err(PlanError::UnsupportedClause("CREATE INDEX"))
    );
}
