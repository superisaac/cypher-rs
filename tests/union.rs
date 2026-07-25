use std::collections::HashSet;

use cypher_rs::*;

#[test]
fn parses_union_and_union_all_markers() {
    let query = parse("RETURN 1 UNION RETURN 2 UNION ALL RETURN 3").unwrap();
    assert_eq!(query.clauses.len(), 5);
    assert!(matches!(
        query.clauses[1],
        Clause::Union(UnionClause { all: false })
    ));
    assert!(matches!(
        query.clauses[3],
        Clause::Union(UnionClause { all: true })
    ));
}

#[test]
fn union_keywords_are_case_insensitive_and_have_boundaries() {
    assert!(parse("RETURN 1 union all RETURN 2").is_ok());
    assert!(parse("RETURN unionValue").is_ok());
    assert!(parse("RETURN 1 UNIONED RETURN 2").is_err());
}

#[test]
fn rejects_leading_trailing_and_repeated_union() {
    assert!(parse("UNION RETURN 1").is_err());
    assert!(parse("RETURN 1 UNION").is_err());
    assert!(parse("RETURN 1 UNION UNION RETURN 2").is_err());
}

#[test]
fn plans_union_and_union_all_as_left_deep_tree() {
    let plan = plan(&parse("RETURN 1 UNION RETURN 2 UNION ALL RETURN 3").unwrap()).unwrap();
    match plan {
        Plan::Union {
            left,
            right,
            all: true,
        } => {
            assert!(matches!(*right, Plan::Project { .. }));
            assert!(matches!(*left, Plan::Union { all: false, .. }));
        }
        other => panic!("expected left-deep Union, got {other:?}"),
    }
}

#[test]
fn planner_rejects_mismatched_projection_counts() {
    let query = parse("RETURN 1 UNION RETURN 2, 3").unwrap();
    assert_eq!(
        plan(&query),
        Err(PlanError::UnionColumnCountMismatch { left: 1, right: 2 })
    );
}

#[test]
fn planner_requires_return_only_for_composed_queries() {
    let single = Query::new(vec![Clause::Match(MatchClause {
        optional: false,
        hints: Vec::new(),
        patterns: vec![Pattern {
            path_variable: None,
            anchor: NodePattern {
                var: Some("n".into()),
                labels: Vec::new(),
                properties: Vec::new(),
                property_map: None,
            },
            chain: Vec::new(),
            shortest: None,
        }],
    })]);
    assert!(matches!(plan(&single), Ok(Plan::Scan { .. })));

    let composed = parse("MATCH (n) UNION RETURN 1").unwrap();
    assert_eq!(plan(&composed), Err(PlanError::UnionBranchWithoutReturn));
}

#[test]
fn semantic_bindings_do_not_leak_between_union_branches() {
    let query = parse("MATCH (left) RETURN left UNION RETURN left").unwrap();
    let report = analyze(&query);
    assert!(report
        .errors()
        .any(|issue| { issue.code == "unbound-variable" && issue.message.contains("`left`") }));
}

#[test]
fn optimizer_pruning_and_cost_handle_union_branches() {
    let plan = plan(&parse("MATCH (a) WHERE a.ok RETURN a UNION ALL MATCH (a) RETURN a").unwrap())
        .unwrap();
    let optimized = optimize(plan);
    assert!(matches!(optimized, Plan::Union { all: true, .. }));
    assert_eq!(output_columns(&optimized), ["a".to_string()].into());
    assert_eq!(
        required_input_columns(&optimized, &HashSet::new()),
        HashSet::new()
    );

    let estimate = estimate(&optimized, &CardinalityCostModel::default());
    assert!(estimate.cardinality > 0.0);
    assert!(estimate.cost > 0.0);
}

#[test]
fn union_display_includes_both_branches_and_mode() {
    let plan = plan(&parse("RETURN 1 UNION ALL RETURN 2").unwrap()).unwrap();
    let display = plan.to_string();
    assert!(display.contains("Union { all: true }"));
    assert_eq!(display.matches("Project").count(), 2);
}
