use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan,
};

#[test]
fn delete_plan_preserves_expressions_and_detach_mode() {
    let logical = plan(&parse("MATCH (n)-[r]-() DELETE r, n").unwrap()).unwrap();
    let Plan::Delete {
        input,
        detach,
        expressions,
    } = &logical
    else {
        panic!("expected Delete");
    };

    assert!(!detach);
    assert!(matches!(input.as_ref(), Plan::Expand { .. }));
    assert_eq!(expressions.len(), 2);
    assert_eq!(
        output_columns(&logical),
        HashSet::from(["n".into(), "r".into()])
    );

    let detached = plan(&parse("MATCH (n) DETACH DELETE n").unwrap()).unwrap();
    assert!(matches!(detached, Plan::Delete { detach: true, .. }));
}

#[test]
fn delete_tracks_expression_dependencies() {
    let logical =
        plan(&parse("MATCH (n), (m) DELETE n, coalesce(m.owner, n.owner)").unwrap()).unwrap();
    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["n".into(), "m".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_delete() {
    let logical = plan(&parse("MATCH (n) DETACH DELETE n").unwrap()).unwrap();
    assert_eq!(optimize(logical.clone()), logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 10_000.0);
    assert_eq!(estimate.cost, 20_000.0);
    assert!(logical
        .to_string()
        .starts_with("Delete { detach: true, expressions: 1 }"));
}
