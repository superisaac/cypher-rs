use cypher_rs::*;

#[test]
fn parses_explain_reference_form() {
    let query = parse("EXPLAIN RETURN 1").unwrap();

    assert_eq!(query.options, [QueryOption::Explain]);
    assert!(matches!(query.clauses[0], Clause::Return(_)));
    assert_eq!(query.option_count(), 1);
}

#[test]
fn explain_can_prefix_schema_commands() {
    let query = parse("EXPLAIN CREATE INDEX ON :Person(name)").unwrap();

    assert_eq!(query.options, [QueryOption::Explain]);
    assert!(matches!(
        query.clauses[0],
        Clause::SchemaCommand(SchemaCommand::CreateIndex { .. })
    ));
}

#[test]
fn explain_precedes_periodic_commit_and_both_are_preserved() {
    let query =
        parse("EXPLAIN USING PERIODIC COMMIT 500 LOAD CSV FROM $source AS row RETURN row").unwrap();

    assert_eq!(
        query.options,
        [
            QueryOption::Explain,
            QueryOption::UsingPeriodicCommit { limit: Some(500) },
        ]
    );
    assert!(parse("USING PERIODIC COMMIT 500 EXPLAIN RETURN 1").is_err());
}

#[test]
fn options_are_preserved_per_statement() {
    let query = parse("EXPLAIN RETURN 1; RETURN 2; EXPLAIN DROP INDEX ON :Person(name)").unwrap();
    let options = query.statement_options().collect::<Vec<_>>();

    assert_eq!(query.statement_count(), 3);
    assert_eq!(query.option_count(), 2);
    assert_eq!(options[0], [QueryOption::Explain]);
    assert!(options[1].is_empty());
    assert_eq!(options[2], [QueryOption::Explain]);
}

#[test]
fn keywords_are_case_insensitive_reserved_and_have_boundaries() {
    assert_eq!(
        parse("explain return 1").unwrap().options,
        [QueryOption::Explain]
    );
    assert!(parse("EXPLAINED RETURN 1").is_err());
    assert!(parse("MATCH (explain) RETURN explain").is_err());
    assert!(parse("MATCH (`explain`) RETURN `explain`").is_ok());
}

#[test]
fn rejects_malformed_or_misplaced_explain() {
    for source in [
        "EXPLAIN",
        "RETURN 1 EXPLAIN",
        "MATCH (n) EXPLAIN RETURN n",
        "EXPLAIN USING PERIODIC RETURN 1",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn planner_wraps_queries_in_explain() {
    let query =
        parse("EXPLAIN USING PERIODIC COMMIT 250 LOAD CSV FROM $source AS row RETURN row").unwrap();
    let plan = plan(&query).unwrap();

    let Plan::Explain { input } = &plan else {
        panic!("expected Explain");
    };
    assert!(matches!(
        input.as_ref(),
        Plan::PeriodicCommit {
            limit: Some(250),
            ..
        }
    ));
    assert!(plan.to_string().starts_with("Explain\n"));
}

#[test]
fn explain_is_transparent_to_plan_analyses() {
    let query = parse("EXPLAIN MATCH (n) RETURN n").unwrap();
    let plan = plan(&query).unwrap();
    let Plan::Explain { input } = &plan else {
        panic!("expected Explain");
    };

    assert_eq!(
        optimize(plan.clone()),
        Plan::Explain {
            input: Box::new(optimize(input.as_ref().clone())),
        }
    );
    assert_eq!(output_columns(&plan), output_columns(input));
    assert_eq!(
        required_input_columns(&plan, &output_columns(&plan)),
        required_input_columns(input, &output_columns(input))
    );
    assert_eq!(
        estimate(&plan, &CardinalityCostModel::default()),
        estimate(input, &CardinalityCostModel::default())
    );
}
