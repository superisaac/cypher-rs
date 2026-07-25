use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan,
};

#[test]
fn create_plan_preserves_patterns_and_input() {
    let query = parse("MATCH (source) CREATE (n {owner: source})-[:OWNS]->(item)").unwrap();
    let logical = plan(&query).unwrap();
    let Plan::Create {
        input,
        unique,
        patterns,
    } = &logical
    else {
        panic!("expected Create");
    };
    assert!(!unique);
    assert_eq!(patterns.len(), 1);
    assert!(matches!(input.as_ref(), Plan::Scan { .. }));
    assert_eq!(
        output_columns(&logical),
        HashSet::from(["source".into(), "n".into(), "item".into()])
    );
}

#[test]
fn create_tracks_property_dependencies() {
    let logical = plan(&parse("MATCH (source) CREATE (n {owner: source})").unwrap()).unwrap();
    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["source".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_create() {
    let logical = plan(&parse("CREATE UNIQUE (n), (m)").unwrap()).unwrap();
    let optimized = optimize(logical.clone());
    assert_eq!(optimized, logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 1.0);
    assert!(estimate.cost > 0.0);
    assert!(logical
        .to_string()
        .starts_with("Create { unique: true, patterns: 2 }"));
}
