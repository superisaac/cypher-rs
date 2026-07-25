use cypher_rs::*;

#[test]
fn parses_periodic_commit_with_limit() {
    let query = parse("USING PERIODIC COMMIT 500 CREATE (n)").unwrap();

    assert_eq!(
        query.options,
        [QueryOption::UsingPeriodicCommit { limit: Some(500) }]
    );
    assert!(matches!(query.clauses[0], Clause::Create(_)));
    assert_eq!(query.option_count(), 1);
    assert_eq!(query.clause_count(), 1);
}

#[test]
fn parses_periodic_commit_without_limit() {
    let query = parse("USING PERIODIC COMMIT CREATE (n)").unwrap();
    assert_eq!(
        query.options,
        [QueryOption::UsingPeriodicCommit { limit: None }]
    );
}

#[test]
fn accepts_radix_limits_and_rejects_invalid_limits() {
    let query = parse("USING PERIODIC COMMIT 0x200 RETURN 1").unwrap();
    assert_eq!(
        query.options,
        [QueryOption::UsingPeriodicCommit { limit: Some(512) }]
    );

    for source in [
        "USING PERIODIC COMMIT -1 RETURN 1",
        "USING PERIODIC COMMIT 1.5 RETURN 1",
        "USING PERIODIC COMMIT $limit RETURN 1",
        "USING PERIODIC COMMIT 18446744073709551616 RETURN 1",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn options_are_preserved_per_statement() {
    let query = parse(
        "USING PERIODIC COMMIT 100 LOAD CSV FROM $a AS row RETURN row; \
         USING PERIODIC COMMIT LOAD CSV FROM $b AS row RETURN row; \
         RETURN 1",
    )
    .unwrap();
    let options = query.statement_options().collect::<Vec<_>>();

    assert_eq!(query.statement_count(), 3);
    assert_eq!(query.option_count(), 2);
    assert_eq!(
        options[0],
        [QueryOption::UsingPeriodicCommit { limit: Some(100) }]
    );
    assert_eq!(
        options[1],
        [QueryOption::UsingPeriodicCommit { limit: None }]
    );
    assert!(options[2].is_empty());
}

#[test]
fn options_must_be_complete_statement_prefixes() {
    for source in [
        "USING RETURN 1",
        "USING PERIODIC RETURN 1",
        "USING PERIODIC COMMIT",
        "RETURN 1 USING PERIODIC COMMIT",
        "USING PERIODIC COMMIT 10 USING SCAN n:Person RETURN 1",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn keywords_are_case_insensitive_and_have_boundaries() {
    let query = parse("using periodic commit 25 return 1").unwrap();
    assert_eq!(
        query.options,
        [QueryOption::UsingPeriodicCommit { limit: Some(25) }]
    );

    assert!(parse("USING PERIODICCOMMIT 25 RETURN 1").is_err());
    assert!(parse("USING PERIODICAL COMMIT 25 RETURN 1").is_err());
    assert!(parse("USING PERIODIC COMMITTED 25 RETURN 1").is_err());
}

#[test]
fn planner_preserves_periodic_commit_option() {
    let query = parse("USING PERIODIC COMMIT 250 LOAD CSV FROM $source AS row RETURN row").unwrap();
    let plan = plan(&query).unwrap();

    let Plan::PeriodicCommit { input, limit } = &plan else {
        panic!("expected PeriodicCommit");
    };
    assert_eq!(*limit, Some(250));
    assert!(matches!(input.as_ref(), Plan::Project { .. }));
    assert!(plan.to_string().contains("PeriodicCommit { limit: 250 }"));
}

#[test]
fn periodic_commit_is_transparent_to_plan_analyses() {
    let query = parse("USING PERIODIC COMMIT LOAD CSV FROM $source AS row RETURN row").unwrap();
    let plan = plan(&query).unwrap();
    let Plan::PeriodicCommit { input, limit } = &plan else {
        panic!("expected PeriodicCommit");
    };

    assert_eq!(*limit, None);
    assert_eq!(
        optimize(plan.clone()),
        Plan::PeriodicCommit {
            input: Box::new(optimize(input.as_ref().clone())),
            limit: None,
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
