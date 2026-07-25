use std::collections::HashSet;

use cypher_rs::*;

fn first_return_expr(query: &str) -> Expr {
    let query = parse(query).unwrap();
    match &query.clauses[0] {
        Clause::Return(return_clause) => return_clause.items[0].expr.clone(),
        clause => panic!("expected RETURN, got {clause:?}"),
    }
}

#[test]
fn parses_simple_case_with_multiple_alternatives_and_else() {
    let expr = first_return_expr(
        "RETURN CASE n.status WHEN 'active' THEN 1 WHEN 'paused' THEN 2 ELSE 0 END",
    );
    match expr {
        Expr::Case {
            operand: Some(operand),
            alternatives,
            else_expr: Some(else_expr),
        } => {
            assert!(matches!(*operand, Expr::Property { ref key, .. } if key == "status"));
            assert_eq!(alternatives.len(), 2);
            assert!(matches!(
                alternatives[0].when,
                Expr::Literal(Literal::String(ref value)) if value == "active"
            ));
            assert!(matches!(*else_expr, Expr::Literal(Literal::Int(0))));
        }
        other => panic!("expected simple CASE, got {other:?}"),
    }
}

#[test]
fn parses_searched_case_without_else() {
    let expr = first_return_expr(
        "RETURN CASE WHEN n.score >= 90 THEN 'A' WHEN n.score >= 80 THEN 'B' END",
    );
    match expr {
        Expr::Case {
            operand: None,
            alternatives,
            else_expr: None,
        } => {
            assert_eq!(alternatives.len(), 2);
            assert!(matches!(
                alternatives[0].when,
                Expr::Binary { op: BinOp::Gte, .. }
            ));
        }
        other => panic!("expected searched CASE, got {other:?}"),
    }
}

#[test]
fn case_is_case_insensitive_and_can_be_nested() {
    let query = "RETURN case when true then CASE 1 WHEN 1 THEN 'yes' END else 'no' end";
    let expr = first_return_expr(query);
    assert!(matches!(
        expr,
        Expr::Case {
            alternatives,
            ..
        } if matches!(alternatives[0].then, Expr::Case { .. })
    ));
}

#[test]
fn case_can_participate_in_larger_expressions() {
    let expr = first_return_expr("RETURN CASE WHEN true THEN 1 ELSE 2 END + 3");
    assert!(matches!(
        expr,
        Expr::Binary {
            op: BinOp::Add,
            lhs,
            ..
        } if matches!(*lhs, Expr::Case { .. })
    ));
}

#[test]
fn rejects_case_without_alternative_or_then_value() {
    assert!(parse("RETURN CASE END").is_err());
    assert!(parse("RETURN CASE WHEN true THEN END").is_err());
}

#[test]
fn semantic_analysis_checks_every_case_expression() {
    let query = parse(
        "MATCH (n) RETURN CASE missing WHEN n.key THEN absent WHEN other.ok THEN n.value ELSE fallback END",
    )
    .unwrap();
    let report = analyze(&query);
    let messages = report
        .errors()
        .map(|issue| issue.message.as_str())
        .collect::<Vec<_>>();
    for variable in ["missing", "absent", "other", "fallback"] {
        assert!(messages.iter().any(|message| message.contains(variable)));
    }
}

#[test]
fn planner_and_pruner_preserve_case_expression_dependencies() {
    let query = parse(
        "MATCH (n) RETURN CASE WHEN n.active THEN n.name ELSE 'inactive' END AS display_name",
    )
    .unwrap();
    let plan = plan(&query).unwrap();
    assert!(matches!(
        plan,
        Plan::Project { ref exprs, .. } if matches!(exprs[0].expr, Expr::Case { .. })
    ));

    let demand = ["display_name".to_string()]
        .into_iter()
        .collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&plan, &demand),
        ["n".to_string()].into()
    );
}
