use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan,
};

#[test]
fn foreach_plan_preserves_local_update_pipeline() {
    let logical =
        plan(&parse("MATCH (n) FOREACH (x IN [1, 2] | SET n.value = x REMOVE n.old)").unwrap())
            .unwrap();
    let Plan::Foreach {
        input,
        variable,
        updates,
        ..
    } = &logical
    else {
        panic!("expected Foreach");
    };

    assert_eq!(variable, "x");
    assert!(matches!(input.as_ref(), Plan::Scan { .. }));
    assert!(
        matches!(updates.as_ref(), Plan::Remove { input, .. } if matches!(input.as_ref(), Plan::Set { .. }))
    );
    assert_eq!(output_columns(&logical), HashSet::from(["n".into()]));
    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["n".into()])
    );
}

#[test]
fn foreach_plans_nested_bodies_without_leaking_iterators() {
    let logical = plan(
        &parse("MATCH (n) FOREACH (x IN [1] | FOREACH (y IN [x] | SET n.value = y))").unwrap(),
    )
    .unwrap();
    let Plan::Foreach { updates, .. } = &logical else {
        panic!("expected outer Foreach");
    };
    assert!(matches!(updates.as_ref(), Plan::Foreach { variable, .. } if variable == "y"));
    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["n".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_foreach() {
    let logical =
        plan(&parse("FOREACH (x IN [1, 2, 3] | CREATE (n {value: x}))").unwrap()).unwrap();
    assert_eq!(optimize(logical.clone()), logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 1.0);
    assert_eq!(estimate.cost, 3.0);
    assert!(logical.to_string().starts_with("Foreach { variable: x"));
}
