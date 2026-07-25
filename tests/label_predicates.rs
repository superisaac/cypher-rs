use std::collections::HashSet;

use cypher_rs::*;

fn return_expr(query: &str) -> Expr {
    let query = parse(query).unwrap();
    let Clause::Return(return_clause) = &query.clauses[0] else {
        panic!("expected RETURN");
    };
    return_clause.items[0].expr.clone()
}

#[test]
fn parses_single_and_multiple_label_predicates() {
    let expression = return_expr("RETURN n:Person:Employee");
    assert!(matches!(
        expression,
        Expr::LabelPredicate { expression, labels }
            if labels == ["Person", "Employee"]
                && matches!(expression.as_ref(), Expr::Variable(name) if name == "n")
    ));
}

#[test]
fn label_predicates_follow_other_postfix_operations() {
    let expression = return_expr("RETURN rows[0].owner:Person");
    assert!(matches!(
        expression,
        Expr::LabelPredicate { expression, labels }
            if labels == ["Person"]
                && matches!(expression.as_ref(), Expr::Property { .. })
    ));
}

#[test]
fn label_predicates_bind_tighter_than_boolean_operators() {
    let expression = return_expr("RETURN n:Person AND n.active");
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::And, lhs, .. }
            if matches!(lhs.as_ref(), Expr::LabelPredicate { .. })
    ));
}

#[test]
fn decodes_escaped_label_names() {
    let expression = return_expr("RETURN n:`User Profile`");
    assert!(matches!(
        expression,
        Expr::LabelPredicate { labels, .. } if labels == ["User Profile"]
    ));
}

#[test]
fn rejects_malformed_or_misordered_label_predicates() {
    for query in ["RETURN n:", "RETURN n::Person", "RETURN n:Person.name"] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_checks_binding_and_schema() {
    struct OnlyPerson;
    impl Schema for OnlyPerson {
        fn has_label(&self, label: &str) -> bool {
            label == "Person"
        }
    }

    let query = parse("MATCH (n) RETURN n:Person:Missing, absent:Person").unwrap();
    let issues = analyze_with(&query, &OnlyPerson);
    let codes = issues.errors().map(|issue| issue.code).collect::<Vec<_>>();
    assert!(codes.contains(&"unknown-label"));
    assert!(codes.contains(&"unbound-variable"));
}

#[test]
fn planner_optimizer_and_pruner_preserve_dependencies() {
    let query = parse("MATCH (n), (m) WHERE n:Person RETURN n:Person AS person").unwrap();
    let logical_plan = plan(&query).unwrap();
    let optimized = optimize(logical_plan.clone());
    assert!(format!("{optimized}").contains("Filter"));

    let demand = ["person".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        ["n".to_string()].into()
    );
}
