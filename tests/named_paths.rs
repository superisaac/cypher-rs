use std::collections::HashSet;

use cypher_rs::*;

#[test]
fn parses_named_paths() {
    let query = parse("MATCH p = (n)-[r:KNOWS]->(m) RETURN p").unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    let pattern = &match_clause.patterns[0];
    assert_eq!(pattern.path_variable.as_deref(), Some("p"));
    assert_eq!(pattern.anchor.var.as_deref(), Some("n"));
    assert_eq!(pattern.chain[0].rel.var.as_deref(), Some("r"));
    assert_eq!(pattern.chain[0].node.var.as_deref(), Some("m"));
}

#[test]
fn parses_named_shortest_paths_and_escaped_names() {
    let query =
        parse("MATCH `best path` = shortestPath((n)-[*1..3]->(m)) RETURN `best path`").unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    let pattern = &match_clause.patterns[0];
    assert_eq!(pattern.path_variable.as_deref(), Some("best path"));
    assert_eq!(pattern.shortest, Some(ShortestPathMode::Single));
}

#[test]
fn rejects_malformed_named_paths() {
    for query in [
        "MATCH p = RETURN p",
        "MATCH = (n) RETURN n",
        "MATCH p == (n) RETURN p",
        "MATCH p = ()-- RETURN p",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_binds_the_path_and_checks_schema() {
    struct OnlyPerson;
    impl Schema for OnlyPerson {
        fn has_label(&self, label: &str) -> bool {
            label == "Person"
        }

        fn has_rel_type(&self, rel_type: &str) -> bool {
            rel_type == "KNOWS"
        }
    }

    let query = parse("MATCH p = (n:Missing)-[:BAD]->(m) RETURN p, n, m").unwrap();
    let report = analyze_with(&query, &OnlyPerson);
    assert!(report.bindings.contains("p"));
    assert!(!report
        .errors()
        .any(|issue| issue.code == "unbound-variable"));
    let codes = report.errors().map(|issue| issue.code).collect::<Vec<_>>();
    assert!(codes.contains(&"unknown-label"));
    assert!(codes.contains(&"unknown-rel-type"));
}

#[test]
fn planner_binds_named_paths_after_shortest_path_selection() {
    let query = parse("MATCH p = allShortestPaths((n)-[*]->(m)) RETURN p").unwrap();
    let Plan::Project { input, .. } = plan(&query).unwrap() else {
        panic!("expected Project");
    };
    let Plan::NamedPath { input, variable } = *input else {
        panic!("expected NamedPath");
    };
    assert_eq!(variable, "p");
    assert!(matches!(*input, Plan::ShortestPath { all: true, .. }));
}

#[test]
fn optimizer_respects_the_path_binding_boundary() {
    let query = parse("MATCH p = (n) WHERE n.active RETURN p").unwrap();
    let Plan::Project { input, .. } = optimize(plan(&query).unwrap()) else {
        panic!("expected Project");
    };
    assert!(matches!(*input, Plan::NamedPath { input, .. }
        if matches!(*input, Plan::Filter { .. })));

    let query = parse("MATCH p = (n) WHERE p IS NOT NULL RETURN p").unwrap();
    let Plan::Project { input, .. } = optimize(plan(&query).unwrap()) else {
        panic!("expected Project");
    };
    assert!(matches!(*input, Plan::Filter { input, .. }
        if matches!(*input, Plan::NamedPath { .. })));
}

#[test]
fn pruning_cost_and_display_handle_named_paths() {
    let named = plan(&parse("MATCH p = (n)-->(m) RETURN p").unwrap()).unwrap();
    let Plan::Project { input, .. } = &named else {
        panic!("expected Project");
    };
    assert_eq!(
        output_columns(input),
        HashSet::from(["n".into(), "m".into(), "p".into()])
    );
    assert!(required_input_columns(input, &HashSet::from(["p".into()])).is_empty());
    assert!(format!("{named}").contains("NamedPath { variable: p }"));

    let unnamed = plan(&parse("MATCH (n)-->(m) RETURN n").unwrap()).unwrap();
    assert!(
        estimate_cost(&named, &CardinalityCostModel::default())
            > estimate_cost(&unnamed, &CardinalityCostModel::default())
    );
}
