use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    MergeActionKind, Plan, SetItem,
};

#[test]
fn merge_plan_preserves_pattern_actions_and_input() {
    let query = parse(
        "MATCH (source) MERGE (n {owner: source})-[:OWNS]->(item) \
         ON CREATE SET n.created_by = source \
         ON MATCH SET item += source.properties",
    )
    .unwrap();
    let logical = plan(&query).unwrap();
    let Plan::Merge {
        input,
        pattern,
        actions,
    } = &logical
    else {
        panic!("expected Merge");
    };

    assert!(matches!(input.as_ref(), Plan::Scan { .. }));
    assert_eq!(pattern.anchor.var.as_deref(), Some("n"));
    assert_eq!(pattern.chain.len(), 1);
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].kind, MergeActionKind::OnCreate);
    assert!(matches!(actions[0].items[0], SetItem::Property { .. }));
    assert_eq!(actions[1].kind, MergeActionKind::OnMatch);
    assert!(matches!(
        actions[1].items[0],
        SetItem::MergeProperties { .. }
    ));
    assert_eq!(
        output_columns(&logical),
        HashSet::from(["source".into(), "n".into(), "item".into()])
    );
}

#[test]
fn merge_tracks_pattern_and_action_dependencies() {
    let logical = plan(
        &parse(
            "MATCH (source), (defaults) \
             MERGE (n {owner: source}) \
             ON CREATE SET n = defaults \
             ON MATCH SET n.last_seen = source.timestamp",
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["source".into(), "defaults".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_merge() {
    let logical = plan(
        &parse("MERGE (n) ON CREATE SET n.created = true ON MATCH SET n.seen = true").unwrap(),
    )
    .unwrap();
    let optimized = optimize(logical.clone());
    assert_eq!(optimized, logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 1.0);
    assert_eq!(estimate.cost, 3.0);
    assert!(logical.to_string().starts_with("Merge { actions: 2 }"));
}
