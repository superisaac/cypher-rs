use std::collections::HashSet;

use cypher_rs::*;

fn return_expr(query: &str) -> Expr {
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
fn parses_unicode_names_across_patterns_and_projections() {
    let query = parse("MATCH (用户:人物)-[关系:认识]->(朋友) RETURN 用户.姓名 AS 显示名").unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    let pattern = &match_clause.patterns[0];
    assert_eq!(pattern.anchor.var.as_deref(), Some("用户"));
    assert_eq!(pattern.anchor.labels, ["人物"]);
    assert_eq!(pattern.chain[0].rel.var.as_deref(), Some("关系"));
    assert_eq!(pattern.chain[0].rel.types, ["认识"]);
    assert_eq!(pattern.chain[0].node.var.as_deref(), Some("朋友"));

    let Clause::Return(return_clause) = &query.clauses[1] else {
        panic!("expected RETURN");
    };
    assert_eq!(return_clause.items[0].alias.as_deref(), Some("显示名"));
}

#[test]
fn supports_multiple_scripts_and_combining_marks() {
    let expression = return_expr("RETURN γραφ.μήκος(данные, नाम, cafe\u{301})");
    assert!(matches!(
        expression,
        Expr::FunctionCall { name, arguments, .. }
            if name == "γραφ.μήκος"
                && matches!(&arguments, FunctionArguments::Expressions(values)
                    if values.as_slice() == [
                        Expr::Variable("данные".into()),
                        Expr::Variable("नाम".into()),
                        Expr::Variable("cafe\u{301}".into()),
                    ])
    ));
}

#[test]
fn supports_connector_starts_and_currency_continuations() {
    let query = parse("RETURN ‿value, price€, total$value, $参数").unwrap();
    let Clause::Return(return_clause) = &query.clauses[0] else {
        panic!("expected RETURN");
    };
    assert_eq!(return_clause.items[0].expr, Expr::Variable("‿value".into()));
    assert_eq!(return_clause.items[1].expr, Expr::Variable("price€".into()));
    assert_eq!(
        return_clause.items[2].expr,
        Expr::Variable("total$value".into())
    );
    assert_eq!(return_clause.items[3].expr, Expr::Param("参数".into()));
}

#[test]
fn unicode_continuations_preserve_keyword_boundaries() {
    assert_eq!(
        return_expr("RETURN MATCH变量"),
        Expr::Variable("MATCH变量".into())
    );
    assert_eq!(
        return_expr("RETURN true€value"),
        Expr::Variable("true€value".into())
    );
    assert!(parse("MATCH (节点) RETURN 节点").is_ok());
}

#[test]
fn rejects_invalid_unescaped_identifier_starts() {
    for query in [
        "RETURN €price",
        "RETURN \u{301}accent",
        "RETURN 😀value",
        "RETURN 1name",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn unicode_names_work_in_procedures_maps_and_named_paths() {
    let query =
        parse("CALL 数据库.查找($参数) YIELD 结果 AS 用户结果 RETURN {名字: 用户结果.姓名}")
            .unwrap();
    let Clause::Call(call) = &query.clauses[0] else {
        panic!("expected CALL");
    };
    assert_eq!(call.name, "数据库.查找");
    assert_eq!(call.yields[0].binding(), "用户结果");

    let query = parse("MATCH 路径 = (起点)-->(终点) RETURN 路径").unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    assert_eq!(
        match_clause.patterns[0].path_variable.as_deref(),
        Some("路径")
    );
}

#[test]
fn unicode_bindings_flow_through_sema_planning_and_pruning() {
    let query = parse("MATCH (用户) RETURN 用户.姓名 AS 显示名").unwrap();
    assert!(!analyze(&query).has_errors());
    let logical_plan = plan(&query).unwrap();
    let demand = HashSet::from(["显示名".to_string()]);
    assert_eq!(
        required_input_columns(&logical_plan, &demand),
        HashSet::from(["用户".to_string()])
    );
}
