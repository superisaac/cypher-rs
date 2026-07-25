use std::collections::HashSet;

use cypher_rs::*;

#[test]
fn parses_node_and_relationship_property_parameters() {
    let query =
        parse("MATCH (n:Person $nodeProps)-[r:KNOWS*1..2 $relProps]->(m $otherProps) RETURN n")
            .unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    let pattern = &match_clause.patterns[0];
    assert!(matches!(
        &pattern.anchor.property_map,
        Some(Expr::Param(name)) if name == "nodeProps"
    ));
    assert!(matches!(
        &pattern.chain[0].rel.property_map,
        Some(Expr::Param(name)) if name == "relProps"
    ));
    assert!(matches!(
        &pattern.chain[0].node.property_map,
        Some(Expr::Param(name)) if name == "otherProps"
    ));
}

#[test]
fn supports_anonymous_elements_and_escaped_parameter_names() {
    let query = parse("MATCH ($`node props`)-[$`rel props`]->() RETURN 1").unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    let pattern = &match_clause.patterns[0];
    assert!(matches!(
        &pattern.anchor.property_map,
        Some(Expr::Param(name)) if name == "node props"
    ));
    assert!(matches!(
        &pattern.chain[0].rel.property_map,
        Some(Expr::Param(name)) if name == "rel props"
    ));
}

#[test]
fn rejects_multiple_or_non_parameter_property_sources() {
    for query in [
        "MATCH (n {id: 1} $props) RETURN n",
        "MATCH (n $props {id: 1}) RETURN n",
        "MATCH (n [1, 2]) RETURN n",
        "MATCH (n $) RETURN n",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_treats_parameters_as_bound_and_checks_schema() {
    struct EmptySchema;
    impl Schema for EmptySchema {
        fn has_label(&self, _label: &str) -> bool {
            false
        }

        fn has_rel_type(&self, _rel_type: &str) -> bool {
            false
        }
    }

    let query = parse("MATCH (n:Missing $nodes)-[:BAD $rels]->(m) RETURN n, m").unwrap();
    let report = analyze_with(&query, &EmptySchema);
    assert!(!report
        .errors()
        .any(|issue| issue.code == "unbound-variable"));
    let codes = report.errors().map(|issue| issue.code).collect::<Vec<_>>();
    assert!(codes.contains(&"unknown-label"));
    assert!(codes.contains(&"unknown-rel-type"));
}

#[test]
fn planner_preserves_dynamic_maps_for_nodes_and_relationships() {
    let query = parse("MATCH (n $nodes)-[r $rels]->(m) RETURN n").unwrap();
    let Plan::Project { input, .. } = plan(&query).unwrap() else {
        panic!("expected Project");
    };
    let Plan::PropertyMapFilter {
        input,
        variable,
        map,
    } = *input
    else {
        panic!("expected relationship PropertyMapFilter");
    };
    assert_eq!(variable, "r");
    assert_eq!(map, Expr::Param("rels".into()));
    let Plan::Expand { input, .. } = *input else {
        panic!("expected Expand");
    };
    assert!(matches!(
        *input,
        Plan::PropertyMapFilter { variable, map, .. }
            if variable == "n" && map == Expr::Param("nodes".into())
    ));
}

#[test]
fn anonymous_dynamic_maps_receive_internal_bindings() {
    let query = parse("MATCH ($nodes)-[$rels]->() RETURN 1").unwrap();
    let rendered = format!("{}", plan(&query).unwrap());
    assert!(rendered.contains("PropertyMapFilter { variable: __node_0"));
    assert!(rendered.contains("PropertyMapFilter { variable: __rel_1"));
}

#[test]
fn optimizer_pruning_cost_and_display_handle_property_map_filters() {
    let dynamic = plan(&parse("MATCH (n $props) WHERE n.active RETURN n").unwrap()).unwrap();
    let Plan::Project { input, .. } = optimize(dynamic.clone()) else {
        panic!("expected Project");
    };
    assert!(matches!(*input, Plan::PropertyMapFilter { input, .. }
        if matches!(*input, Plan::Filter { .. })));

    let Plan::Project { input, .. } = &dynamic else {
        panic!("expected Project");
    };
    assert_eq!(output_columns(input), HashSet::from(["n".into()]));
    assert_eq!(
        required_input_columns(input, &HashSet::new()),
        HashSet::from(["n".into()])
    );
    assert!(format!("{dynamic}").contains("PropertyMapFilter"));

    let plain = plan(&parse("MATCH (n) RETURN n").unwrap()).unwrap();
    let model = CardinalityCostModel::default();
    assert!(estimate_cost(&dynamic, &model) > estimate_cost(&plain, &model));
}
