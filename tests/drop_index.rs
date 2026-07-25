use cypher_rs::*;

#[test]
fn parses_drop_index_reference_form() {
    let query = parse("DROP INDEX ON :Foo(bar)").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::DropIndex { label, properties })
            if label == "Foo" && properties == &["bar"]
    ));
}

#[test]
fn parses_composite_indexes_and_escaped_names() {
    let query = parse("DROP INDEX ON :`User Profile`(`first name`, `drop`)").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::DropIndex { label, properties })
            if label == "User Profile" && properties == &["first name", "drop"]
    ));
}

#[test]
fn keywords_are_case_insensitive_and_comments_are_supported() {
    let query = parse("/* drop! */drop index on /* a label */ :Person(name, age)").unwrap();
    assert!(matches!(
        query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::DropIndex { .. })
    ));

    assert!(parse("DROPPED INDEX ON :Person(name)").is_err());
    assert!(parse("DROP INDEXED ON :Person(name)").is_err());
}

#[test]
fn rejects_malformed_drop_index_commands() {
    for source in [
        "DROP INDEX",
        "DROP INDEX ON",
        "DROP INDEX ON Person(name)",
        "DROP INDEX ON :(name)",
        "DROP INDEX ON :Person",
        "DROP INDEX ON :Person()",
        "DROP INDEX ON :Person(name,)",
        "DROP INDEX ON :Person(name",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn drop_index_is_an_isolated_schema_statement() {
    for source in [
        "DROP INDEX ON :Person(name) RETURN 1",
        "MATCH (n) DROP INDEX ON :Person(name)",
        "USING PERIODIC COMMIT DROP INDEX ON :Person(name)",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn create_and_drop_indexes_can_be_separate_statements() {
    let query =
        parse("CREATE INDEX ON :Person(name); DROP INDEX ON :Person(name); RETURN 1").unwrap();
    let statements = query.statements().collect::<Vec<_>>();

    assert_eq!(query.statement_count(), 3);
    assert!(matches!(
        statements[0],
        [Clause::SchemaCommand(SchemaCommand::CreateIndex { .. })]
    ));
    assert!(matches!(
        statements[1],
        [Clause::SchemaCommand(SchemaCommand::DropIndex { .. })]
    ));
    assert!(matches!(statements[2], [Clause::Return(_)]));
}

#[test]
fn semantic_analysis_accepts_drop_index_without_bindings() {
    let query = parse("DROP INDEX ON :Person(name, email)").unwrap();
    let report = analyze(&query);

    assert!(!report.has_errors());
    assert!(report.bindings.is_empty());
}

#[test]
fn planner_supports_drop_index() {
    let query = parse("DROP INDEX ON :Person(name)").unwrap();
    assert!(matches!(
        plan(&query).unwrap(),
        Plan::DropIndex { label, properties }
            if label == "Person" && properties == ["name"]
    ));
}
