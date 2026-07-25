use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan,
};

#[test]
fn create_index_plan_preserves_label_and_properties() {
    let logical =
        plan(&parse("CREATE INDEX ON :`User Profile`(`first name`, email)").unwrap()).unwrap();
    assert!(matches!(
        &logical,
        Plan::CreateIndex { label, properties }
            if label == "User Profile" && properties == &["first name", "email"]
    ));
    assert!(output_columns(&logical).is_empty());
    assert!(required_input_columns(&logical, &HashSet::new()).is_empty());
}

#[test]
fn query_options_can_wrap_create_index() {
    let logical = plan(&parse("EXPLAIN CREATE INDEX ON :Person(name)").unwrap()).unwrap();
    assert!(matches!(
        logical,
        Plan::Explain { input } if matches!(input.as_ref(), Plan::CreateIndex { .. })
    ));
}

#[test]
fn optimizer_cost_and_display_handle_create_index() {
    let logical = plan(&parse("CREATE INDEX ON :Person(name, email)").unwrap()).unwrap();
    assert_eq!(optimize(logical.clone()), logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 0.0);
    assert_eq!(estimate.cost, 1.0);
    assert_eq!(
        logical.to_string(),
        "CreateIndex { label: Person, properties: [name, email] }\n"
    );
}
