use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan,
};

#[test]
fn drop_index_plan_preserves_label_and_properties() {
    let logical =
        plan(&parse("DROP INDEX ON :`User Profile`(`first name`, email)").unwrap()).unwrap();
    assert!(matches!(
        &logical,
        Plan::DropIndex { label, properties }
            if label == "User Profile" && properties == &["first name", "email"]
    ));
    assert!(output_columns(&logical).is_empty());
    assert!(required_input_columns(&logical, &HashSet::new()).is_empty());
}

#[test]
fn query_options_can_wrap_drop_index() {
    let logical = plan(&parse("PROFILE DROP INDEX ON :Person(name)").unwrap()).unwrap();
    assert!(matches!(
        logical,
        Plan::Profile { input } if matches!(input.as_ref(), Plan::DropIndex { .. })
    ));
}

#[test]
fn optimizer_cost_and_display_handle_drop_index() {
    let logical = plan(&parse("DROP INDEX ON :Person(name, email)").unwrap()).unwrap();
    assert_eq!(optimize(logical.clone()), logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 0.0);
    assert_eq!(estimate.cost, 1.0);
    assert_eq!(
        logical.to_string(),
        "DropIndex { label: Person, properties: [name, email] }\n"
    );
}
