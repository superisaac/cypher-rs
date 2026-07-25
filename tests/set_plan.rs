use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan, SetItem,
};

#[test]
fn set_plan_preserves_all_item_forms_and_input() {
    let logical = plan(
        &parse(
            "MATCH (n), (source), (replacement) \
             SET n.name = source.name, n = replacement, n += source, n:Active:Current",
        )
        .unwrap(),
    )
    .unwrap();
    let Plan::Set { input, items } = &logical else {
        panic!("expected Set");
    };

    assert!(matches!(input.as_ref(), Plan::Cartesian { .. }));
    assert_eq!(items.len(), 4);
    assert!(matches!(items[0], SetItem::Property { .. }));
    assert!(matches!(items[1], SetItem::AllProperties { .. }));
    assert!(matches!(items[2], SetItem::MergeProperties { .. }));
    assert!(matches!(
        &items[3],
        SetItem::Labels { labels, .. } if labels == &["Active", "Current"]
    ));
    assert_eq!(
        output_columns(&logical),
        HashSet::from(["n".into(), "source".into(), "replacement".into()])
    );
}

#[test]
fn set_tracks_target_and_value_dependencies() {
    let logical = plan(
        &parse(
            "MATCH (n), (source), (replacement) \
             SET n.profile.name = source.name, n = replacement, n:Active",
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["n".into(), "source".into(), "replacement".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_set() {
    let logical = plan(&parse("MATCH (n) SET n.active = true, n:Active").unwrap()).unwrap();
    let optimized = optimize(logical.clone());
    assert_eq!(optimized, logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 10_000.0);
    assert_eq!(estimate.cost, 30_000.0);
    assert!(logical.to_string().starts_with("Set { items: 2 }"));
}
