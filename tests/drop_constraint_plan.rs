use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Expr, Plan,
};

#[test]
fn drop_node_constraint_plan_preserves_target_expression_and_mode() {
    let logical = plan(
        &parse("DROP CONSTRAINT ON (`node var`:`User Profile`) ASSERT `node var`.id IS UNIQUE")
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        &logical,
        Plan::DropNodeConstraint {
            variable,
            label,
            expression: Expr::Property { key, .. },
            unique: true,
        } if variable == "node var" && label == "User Profile" && key == "id"
    ));
    assert!(output_columns(&logical).is_empty());
    assert!(required_input_columns(&logical, &HashSet::new()).is_empty());
}

#[test]
fn drop_relationship_constraint_plan_preserves_target_and_expression() {
    let logical =
        plan(&parse("EXPLAIN DROP CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since)").unwrap())
            .unwrap();
    let Plan::Explain { input } = logical else {
        panic!("expected Explain");
    };
    assert!(matches!(
        input.as_ref(),
        Plan::DropRelationshipConstraint {
            variable,
            relationship_type,
            expression: Expr::FunctionCall { name, .. },
        } if variable == "r" && relationship_type == "KNOWS" && name == "exists"
    ));
}

#[test]
fn optimizer_cost_and_display_handle_drop_constraints() {
    let logical =
        plan(&parse("DROP CONSTRAINT ON (n:Person) ASSERT exists(n.email)").unwrap()).unwrap();
    assert_eq!(optimize(logical.clone()), logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 0.0);
    assert_eq!(estimate.cost, 1.0);
    assert!(logical
        .to_string()
        .starts_with("DropNodeConstraint { variable: n, label: Person"));
}
