use cypher_rs::*;

fn first_pattern(query: &str) -> Pattern {
    let query = parse(query).unwrap();
    match &query.clauses[0] {
        Clause::Match(match_clause) => match_clause.patterns[0].clone(),
        clause => panic!("expected MATCH, got {clause:?}"),
    }
}

#[test]
fn parses_all_variable_length_range_forms() {
    let cases = [
        ("*", None, None),
        ("*3", Some(3), Some(3)),
        ("*1..5", Some(1), Some(5)),
        ("*2..", Some(2), None),
        ("*..4", None, Some(4)),
    ];
    for (syntax, start, end) in cases {
        let query = format!("MATCH (a)-[:KNOWS{syntax}]->(b) RETURN b");
        let pattern = first_pattern(&query);
        assert_eq!(
            pattern.chain[0].rel.range,
            Some(RelationshipRange { start, end }),
            "range for {syntax}"
        );
    }
}

#[test]
fn fixed_relationship_has_no_range() {
    let pattern = first_pattern("MATCH (a)-[:KNOWS]->(b) RETURN b");
    assert_eq!(pattern.chain[0].rel.range, None);
}

#[test]
fn semantic_analysis_rejects_descending_range() {
    let query = parse("MATCH (a)-[:KNOWS*5..2]->(b) RETURN b").unwrap();
    let report = analyze(&query);
    assert!(report
        .errors()
        .any(|issue| issue.code == "invalid-relationship-range"));
}

#[test]
fn parses_shortest_path_modes() {
    let single = first_pattern("MATCH shortestPath((a)-[:ROAD*]->(b)) RETURN b");
    assert_eq!(single.shortest, Some(ShortestPathMode::Single));

    let all = first_pattern("MATCH allShortestPaths((a)-[:ROAD*1..5]->(b)) RETURN b");
    assert_eq!(all.shortest, Some(ShortestPathMode::All));
}

#[test]
fn shortest_path_keywords_are_case_insensitive() {
    let pattern = first_pattern("MATCH ShOrTeStPaTh((a)-[*]->(b)) RETURN b");
    assert_eq!(pattern.shortest, Some(ShortestPathMode::Single));
}

#[test]
fn planner_preserves_range_and_wraps_shortest_path() {
    let query = parse("MATCH shortestPath((a)-[:ROAD*1..5]->(b)) RETURN b").unwrap();
    let plan = plan(&query).unwrap();
    match plan {
        Plan::Project { input, .. } => match *input {
            Plan::ShortestPath { input, all: false } => match *input {
                Plan::Expand { range, .. } => assert_eq!(
                    range,
                    Some(RelationshipRange {
                        start: Some(1),
                        end: Some(5),
                    })
                ),
                other => panic!("expected Expand, got {other:?}"),
            },
            other => panic!("expected ShortestPath, got {other:?}"),
        },
        other => panic!("expected Project, got {other:?}"),
    }
}

#[test]
fn optimizer_pruning_cost_and_display_handle_shortest_paths() {
    let query = parse("MATCH allShortestPaths((a)-[:ROAD*2]->(b)) RETURN b").unwrap();
    let raw = plan(&query).unwrap();
    let optimized = optimize(raw);
    assert!(format!("{optimized}").contains("ShortestPath { all: true }"));
    assert!(format!("{optimized}").contains("range: *2"));
    assert_eq!(output_columns(&optimized), ["b".to_string()].into());

    let estimate = estimate(
        &optimized,
        &CardinalityCostModel::default().with_rel("ROAD", 2.0),
    );
    assert!(estimate.cardinality > 0.0);
    assert!(estimate.cost > 0.0);
}

#[test]
fn cost_model_accounts_for_relationship_length() {
    let one_hop = plan(&parse("MATCH (a)-[:ROAD*1]->(b) RETURN b").unwrap()).unwrap();
    let two_hops = plan(&parse("MATCH (a)-[:ROAD*2]->(b) RETURN b").unwrap()).unwrap();
    let model = CardinalityCostModel::default().with_rel("ROAD", 2.0);
    assert!(estimate(&two_hops, &model).cardinality > estimate(&one_hop, &model).cardinality);
}
