use cypher_rs::{parse, plan, Clause, Plan, QueryOption};

#[test]
fn parses_version_and_settings() {
    let query = parse("CYPHER 3.5 runtime=slotted planner=cost RETURN 1").unwrap();
    assert_eq!(
        query.options,
        [QueryOption::Cypher {
            version: Some("3.5".into()),
            settings: vec![
                ("runtime".into(), "slotted".into()),
                ("planner".into(), "cost".into()),
            ],
        }]
    );
}

#[test]
fn accepts_version_only_and_settings_only() {
    let version = parse("CYPHER 5 RETURN 1").unwrap();
    assert_eq!(
        version.options,
        [QueryOption::Cypher {
            version: Some("5".into()),
            settings: vec![],
        }]
    );

    let settings = parse("cypher runtime=interpreted RETURN 1").unwrap();
    assert_eq!(
        settings.options,
        [QueryOption::Cypher {
            version: None,
            settings: vec![("runtime".into(), "interpreted".into())],
        }]
    );
}

#[test]
fn composes_with_other_statement_and_query_options() {
    let query = parse(
        "EXPLAIN CYPHER 3.5 runtime=slotted USING PERIODIC COMMIT 10 LOAD CSV FROM 'x' AS row RETURN row",
    )
    .unwrap();
    assert!(matches!(query.options[0], QueryOption::Explain));
    assert!(matches!(query.options[1], QueryOption::Cypher { .. }));
    assert!(matches!(
        query.options[2],
        QueryOption::UsingPeriodicCommit { limit: Some(10) }
    ));

    let Plan::Explain { input } = plan(&query).unwrap() else {
        panic!("expected Explain");
    };
    assert!(matches!(*input, Plan::PeriodicCommit { .. }));
}

#[test]
fn supports_schema_commands_and_multiple_statements() {
    let query =
        parse("CYPHER 3.5 CREATE INDEX ON :User(name); CYPHER runtime=slotted RETURN 1").unwrap();
    assert!(matches!(query.clauses[0], Clause::SchemaCommand(_)));
    assert_eq!(query.statement_options().count(), 2);
    assert_eq!(query.option_count(), 2);
}

#[test]
fn rejects_incomplete_or_misplaced_options() {
    for source in [
        "CYPHER RETURN 1",
        "CYPHER 3. RETURN 1",
        "CYPHER runtime= RETURN 1",
        "CYPHER =slotted RETURN 1",
        "CYPHER 3.5 5 RETURN 1",
        "CYPHER runtime=slotted 5 RETURN 1",
        "RETURN 1 CYPHER 5",
        "USING PERIODIC COMMIT CYPHER 5 RETURN 1",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}
