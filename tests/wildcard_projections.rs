use std::collections::HashSet;

use cypher_rs::*;

fn return_clause(input: &str) -> ReturnClause {
    let query = parse(input).unwrap();
    query
        .clauses
        .into_iter()
        .find_map(|clause| match clause {
            Clause::Return(return_clause) => Some(return_clause),
            _ => None,
        })
        .expect("RETURN clause")
}

#[test]
fn parses_return_wildcard_and_mixed_projection() {
    let wildcard = return_clause("MATCH (n) RETURN *");
    assert!(wildcard.include_existing);
    assert!(wildcard.items.is_empty());

    let mixed = return_clause("MATCH (n) RETURN *, n.name AS name");
    assert!(mixed.include_existing);
    assert_eq!(mixed.items.len(), 1);
    assert_eq!(mixed.items[0].alias.as_deref(), Some("name"));
}

#[test]
fn supports_distinct_wildcards_for_return_and_with() {
    let returned = return_clause("MATCH (n) RETURN DISTINCT *, n.name");
    assert!(returned.distinct);
    assert!(returned.include_existing);

    let query = parse("MATCH (n) WITH DISTINCT *, n.name AS name RETURN n, name").unwrap();
    let Clause::With(with_clause) = &query.clauses[1] else {
        panic!("expected WITH clause");
    };
    assert!(with_clause.distinct);
    assert!(with_clause.include_existing);
    assert_eq!(with_clause.items.len(), 1);
}

#[test]
fn wildcard_must_be_the_first_projection() {
    for input in [
        "RETURN n, *",
        "RETURN * AS everything",
        "RETURN *, *",
        "WITH n, * RETURN n",
    ] {
        assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
    }
}

#[test]
fn with_wildcard_preserves_semantic_scope() {
    let query = parse("MATCH (n) WITH *, n.name AS name RETURN n, name").unwrap();
    let report = analyze(&query);
    assert!(
        !report.has_errors(),
        "unexpected semantic issues: {:?}",
        report.issues
    );
}

#[test]
fn planner_and_optimizer_preserve_existing_columns_flag() {
    let query = parse("MATCH (n) RETURN *, n.name AS name").unwrap();
    let planned = plan(&query).unwrap();
    assert!(matches!(
        planned,
        Plan::Project {
            include_existing: true,
            ..
        }
    ));
    assert!(planned.to_string().contains("exprs: [*,"));

    assert!(matches!(
        optimize(planned),
        Plan::Project {
            include_existing: true,
            ..
        }
    ));
}

#[test]
fn column_analysis_includes_wildcard_and_explicit_aliases() {
    let planned = plan(&parse("MATCH (n)-[r]->(m) RETURN *, n.name AS name").unwrap()).unwrap();
    assert_eq!(
        output_columns(&planned),
        ["n", "r", "m", "name"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(
        required_input_columns(&planned, &HashSet::new()),
        ["n", "r", "m"].into_iter().map(str::to_string).collect()
    );
}

#[test]
fn union_counts_columns_expanded_from_wildcards() {
    let compatible = parse("MATCH (a) RETURN * UNION MATCH (b) RETURN *").unwrap();
    assert!(plan(&compatible).is_ok());

    let mismatched = parse("MATCH (a), (b) RETURN * UNION MATCH (c) RETURN *").unwrap();
    assert_eq!(
        plan(&mismatched),
        Err(PlanError::UnionColumnCountMismatch { left: 2, right: 1 })
    );
}
