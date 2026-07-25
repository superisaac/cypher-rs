use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan, RemoveItem,
};

#[test]
fn remove_plan_preserves_property_and_label_items() {
    let logical =
        plan(&parse("MATCH (n), (m) REMOVE n.profile.name, m:Old:Legacy").unwrap()).unwrap();
    let Plan::Remove { input, items } = &logical else {
        panic!("expected Remove");
    };

    assert!(matches!(input.as_ref(), Plan::Cartesian { .. }));
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], RemoveItem::Property(_)));
    assert!(matches!(
        &items[1],
        RemoveItem::Labels { variable, labels }
            if variable == "m" && labels == &["Old", "Legacy"]
    ));
    assert_eq!(
        output_columns(&logical),
        HashSet::from(["n".into(), "m".into()])
    );
}

#[test]
fn remove_tracks_property_and_label_targets() {
    let logical = plan(&parse("MATCH (n), (m) REMOVE n.profile.name, m:Old").unwrap()).unwrap();

    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["n".into(), "m".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_remove() {
    let logical = plan(&parse("MATCH (n) REMOVE n.name, n:Old").unwrap()).unwrap();
    let optimized = optimize(logical.clone());
    assert_eq!(optimized, logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 10_000.0);
    assert_eq!(estimate.cost, 30_000.0);
    assert!(logical.to_string().starts_with("Remove { items: 2 }"));
}
