use cypher_rs::{parse, plan, Plan, QueryOption};

#[test]
fn parses_profile_and_preserves_statement_options() {
    let query = parse("PROFILE RETURN 1").unwrap();
    assert_eq!(query.options, [QueryOption::Profile]);
    assert_eq!(query.option_count(), 1);
}

#[test]
fn profile_prefixes_schema_commands() {
    let query = parse("profile create index on :User(name)").unwrap();
    assert_eq!(query.options, [QueryOption::Profile]);
    assert!(matches!(
        query.clauses[0],
        cypher_rs::Clause::SchemaCommand(_)
    ));
}

#[test]
fn profile_precedes_periodic_commit() {
    let query =
        parse("PROFILE USING PERIODIC COMMIT 25 LOAD CSV FROM 'x' AS row RETURN row").unwrap();
    assert_eq!(
        query.options,
        [
            QueryOption::Profile,
            QueryOption::UsingPeriodicCommit { limit: Some(25) },
        ]
    );
    let planned = plan(&query).unwrap();
    let Plan::Profile { input } = planned else {
        panic!("expected Profile")
    };
    assert!(matches!(*input, Plan::PeriodicCommit { .. }));
}

#[test]
fn profile_is_transparent_to_plan_analyses() {
    let query = parse("PROFILE MATCH (n) RETURN n").unwrap();
    let planned = plan(&query).unwrap();
    let optimized = cypher_rs::optimize(planned.clone());
    assert_eq!(
        cypher_rs::output_columns(&planned),
        cypher_rs::output_columns(&optimized)
    );
    let model = cypher_rs::CardinalityCostModel::default();
    assert_eq!(
        cypher_rs::estimate_cost(&planned, &model),
        cypher_rs::estimate_cost(&optimized, &model)
    );
}

#[test]
fn profile_is_reserved_and_misplaced_forms_are_rejected() {
    assert!(parse("MATCH (n) RETURN n.profile").is_ok());
    assert!(parse("USING PERIODIC COMMIT PROFILE RETURN 1").is_err());
    assert!(parse("RETURN 1 PROFILE").is_err());
    assert!(parse("RETURN profile").is_ok());
}
