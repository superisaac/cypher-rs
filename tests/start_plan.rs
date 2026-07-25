use std::collections::HashSet;

use cypher_rs::{
    estimate, optimize, output_columns, parse, plan, required_input_columns, CardinalityCostModel,
    Plan, StartEntity, StartLookup,
};

#[test]
fn start_plan_preserves_all_lookup_forms_and_predicate() {
    let logical = plan(
        &parse(
            "START n=node(1, 2), r=relationship(*), \
             indexed=node:users(name = 'Ada') \
             WHERE n.active = true",
        )
        .unwrap(),
    )
    .unwrap();
    let Plan::Start {
        input,
        points,
        predicate,
    } = &logical
    else {
        panic!("expected Start");
    };

    assert!(matches!(input.as_ref(), Plan::Empty));
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].entity, StartEntity::Node);
    assert!(matches!(&points[0].lookup, StartLookup::Ids(ids) if ids == &[1, 2]));
    assert_eq!(points[1].entity, StartEntity::Relationship);
    assert!(matches!(points[1].lookup, StartLookup::All));
    assert!(matches!(points[2].lookup, StartLookup::Index { .. }));
    assert!(predicate.is_some());
    assert_eq!(
        output_columns(&logical),
        HashSet::from(["n".into(), "r".into(), "indexed".into()])
    );
}

#[test]
fn start_tracks_predicate_external_dependencies() {
    let logical =
        plan(&parse("START n=node:users(owner = $source) WHERE n.active = flag").unwrap()).unwrap();
    assert_eq!(
        required_input_columns(&logical, &HashSet::new()),
        HashSet::from(["flag".into()])
    );
}

#[test]
fn optimizer_cost_and_display_handle_start() {
    let logical = plan(&parse("START n=node(1, 2)").unwrap()).unwrap();
    assert_eq!(optimize(logical.clone()), logical);
    let estimate = estimate(&logical, &CardinalityCostModel::default());
    assert_eq!(estimate.cardinality, 2.0);
    assert_eq!(estimate.cost, 2.0);
    assert!(logical
        .to_string()
        .starts_with("Start { points: 1, predicate: false }"));
}
