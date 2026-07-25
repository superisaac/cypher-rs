use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Expr, Plan,
};

#[test]
fn unwind_plan_preserves_expression_alias_and_input() {
    let logical = plan(&parse("MATCH (n) UNWIND n.items AS item").unwrap()).unwrap();
    let Plan::Unwind {
        input,
        expression,
        alias,
    } = &logical
    else {
        panic!("expected Unwind");
    };

    assert!(matches!(input.as_ref(), Plan::Scan { .. }));
    assert_eq!(alias, "item");
    assert!(matches!(expression, Expr::Property { key, .. } if key == "items"));
    assert_eq!(
        output_columns(&logical),
        HashSet::from(["n".into(), "item".into()])
    );
}

#[test]
fn unwind_replaces_alias_demand_with_expression_dependencies() {
    let logical = plan(&parse("MATCH (n) UNWIND n.items AS item").unwrap()).unwrap();
    assert_eq!(
        required_input_columns(&logical, &HashSet::from(["item".into()])),
        HashSet::from(["n".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_unwind() {
    let logical = plan(&parse("UNWIND [1, 2, 3] AS item").unwrap()).unwrap();
    assert_eq!(optimize(logical.clone()), logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 3.0);
    assert_eq!(estimate.cost, 3.0);
    assert!(logical.to_string().contains("alias: item"));
}
