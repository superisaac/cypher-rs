use cypher_rs::{analyze, parse, plan, Clause, Expr, Literal, PlanError, StartEntity, StartLookup};

fn start_clause(source: &str) -> cypher_rs::StartClause {
    let query = parse(source).unwrap();
    let Clause::Start(start) = &query.clauses[0] else {
        panic!("expected START");
    };
    start.clone()
}

#[test]
fn parses_node_and_relationship_id_lookups() {
    let start = start_clause("START n=node(65, 0), r=relationship(3) RETURN n, r");
    assert_eq!(start.points.len(), 2);
    assert_eq!(start.points[0].entity, StartEntity::Node);
    assert_eq!(start.points[0].lookup, StartLookup::Ids(vec![65, 0]));
    assert_eq!(start.points[1].entity, StartEntity::Relationship);
    assert_eq!(start.points[1].lookup, StartLookup::Ids(vec![3]));
}

#[test]
fn parses_all_entity_scans() {
    let start = start_clause("START n=node(*), r=rel(*) RETURN n, r");
    assert_eq!(start.points[0].lookup, StartLookup::All);
    assert_eq!(start.points[1].lookup, StartLookup::All);
}

#[test]
fn parses_index_lookup_and_query_forms() {
    let start =
        start_clause("START n=node:users(name = 'Ada'), r=rel:relationships($query) RETURN n, r");
    assert_eq!(
        start.points[0].lookup,
        StartLookup::Index {
            name: "users".into(),
            property: Some("name".into()),
            value: Expr::Literal(Literal::String("Ada".into())),
        }
    );
    assert_eq!(
        start.points[1].lookup,
        StartLookup::Index {
            name: "relationships".into(),
            property: None,
            value: Expr::Param("query".into()),
        }
    );
}

#[test]
fn start_predicate_uses_bound_points() {
    let query = parse("START n=node(*) WHERE n.active = true RETURN n").unwrap();
    let Clause::Start(start) = &query.clauses[0] else {
        panic!("expected START");
    };
    assert!(start.predicate.is_some());
    let report = analyze(&query);
    assert!(!report.has_errors(), "{:?}", report.issues);
    assert!(report.bindings.contains("n"));
}

#[test]
fn supports_escaped_names_comments_and_case_insensitive_keywords() {
    let start = start_clause(
        "start `start` = NoDe:`user index`(`display name` = $value) /* c */ return `start`",
    );
    assert_eq!(start.points[0].variable, "start");
    assert_eq!(start.points[0].entity, StartEntity::Node);
    assert!(parse("RETURN start").is_ok());
}

#[test]
fn planner_reports_start_as_unsupported() {
    let query = parse("START n=node(*) RETURN n").unwrap();
    assert_eq!(plan(&query), Err(PlanError::UnsupportedClause("START")));
}

#[test]
fn rejects_malformed_or_misplaced_start() {
    for source in [
        "START",
        "START n=node() RETURN n",
        "START n=node:idx() RETURN n",
        "START n=node:idx(name=1) RETURN n",
        "START n=node(1,) RETURN n",
        "MATCH (n) START m=node(*) RETURN n, m",
        "START n=node(*) START m=node(*) RETURN n, m",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}
