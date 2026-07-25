use std::collections::HashSet;

use cypher_rs::*;

fn first_return_expr(query: &str) -> Expr {
    let query = parse(query).unwrap();
    let return_clause = query
        .clauses
        .iter()
        .find_map(|clause| match clause {
            Clause::Return(return_clause) => Some(return_clause),
            _ => None,
        })
        .expect("expected RETURN");
    return_clause.items[0].expr.clone()
}

#[test]
fn block_comments_can_separate_tokens_without_whitespace() {
    let query = parse("MATCH/* scan */(n)/* project */RETURN/* value */n").unwrap();
    assert!(matches!(&query.clauses[0], Clause::Match(_)));
    assert!(matches!(&query.clauses[1], Clause::Return(_)));
}

#[test]
fn block_comments_can_span_lines() {
    let query =
        parse("MATCH (n) /* first line\nsecond line\r\nthird line */ WHERE n.active RETURN n")
            .unwrap();
    assert_eq!(query.clauses.len(), 3);
}

#[test]
fn comments_work_between_expression_and_pattern_operators() {
    let expression = first_return_expr("RETURN 1/* lhs */+/* rhs */2 * /* factor */3");
    assert!(matches!(
        expression,
        Expr::Binary { op: BinOp::Add, rhs, .. }
            if matches!(rhs.as_ref(), Expr::Binary { op: BinOp::Mul, .. })
    ));

    let query = parse("MATCH (a)-/* relationship */[:KNOWS]/* arrow */->(b) RETURN a");
    assert!(query.is_ok());
}

#[test]
fn empty_and_symbol_heavy_block_comments_are_accepted() {
    assert!(parse("RETURN/**/1 /* // `quotes` * / symbols +- */").is_ok());
    assert!(parse("RETURN 1 /* trailing comment */").is_ok());
}

#[test]
fn comment_markers_inside_literals_and_escaped_names_are_preserved() {
    assert_eq!(
        first_return_expr("RETURN '/* not a comment */ // text'"),
        Expr::Literal(Literal::String("/* not a comment */ // text".into()))
    );
    assert_eq!(
        first_return_expr("RETURN `/* identifier */`"),
        Expr::Variable("/* identifier */".into())
    );
}

#[test]
fn rejects_unterminated_block_comments() {
    for query in [
        "RETURN 1 /*",
        "MATCH (n) /* unterminated\nRETURN n",
        "/* only a comment",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn comments_do_not_affect_sema_planning_or_pruning() {
    let query = parse("MATCH/* bind */(n) WHERE/* filter */n.active RETURN/* out */n").unwrap();
    assert!(!analyze(&query).has_errors());
    let logical_plan = plan(&query).unwrap();
    let demand = HashSet::from(["n".to_string()]);
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        HashSet::from(["n".to_string()])
    );
}
