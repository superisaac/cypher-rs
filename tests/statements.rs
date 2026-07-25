use cypher_rs::*;

#[test]
fn parses_optional_trailing_semicolon() {
    let query = parse("RETURN 1;").unwrap();
    assert_eq!(query.statement_count(), 1);
    assert_eq!(query.clause_count(), 1);
    assert!(query.additional_statements.is_empty());
}

#[test]
fn preserves_multiple_statement_boundaries() {
    let query = parse("MATCH (n) RETURN n; RETURN 2; RETURN 3;").unwrap();
    let statements: Vec<_> = query.statements().collect();
    assert_eq!(statements.len(), 3);
    assert_eq!(statements[0].len(), 2);
    assert_eq!(statements[1].len(), 1);
    assert_eq!(statements[2].len(), 1);
    assert!(matches!(statements[0][0], Clause::Match(_)));
    assert!(matches!(statements[1][0], Clause::Return(_)));
}

#[test]
fn union_stays_within_its_statement() {
    let query = parse("RETURN 1 UNION ALL RETURN 2; RETURN 3").unwrap();
    assert_eq!(query.statement_count(), 2);
    assert_eq!(query.clauses.len(), 3);
    assert!(matches!(
        query.clauses[1],
        Clause::Union(UnionClause { all: true })
    ));
    assert_eq!(query.additional_statements[0].len(), 1);
}

#[test]
fn accepts_whitespace_and_comments_around_semicolons() {
    let query = parse("RETURN ';' /* before */ ; // after\n RETURN 2 ; /* trailing */").unwrap();
    assert_eq!(query.statement_count(), 2);
    assert_eq!(query.clause_count(), 2);
}

#[test]
fn rejects_empty_statements() {
    for input in [";", "; RETURN 1", "RETURN 1;;RETURN 2", "RETURN 1;;"] {
        assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
    }
}

#[test]
fn semantic_bindings_do_not_leak_between_statements() {
    let query = parse("MATCH (n) RETURN n; RETURN n").unwrap();
    let report = analyze(&query);
    assert!(report
        .errors()
        .any(|issue| { issue.code == "unbound-variable" && issue.message.contains("`n`") }));
}

#[test]
fn planner_explicitly_rejects_multiple_statements() {
    let query = parse("RETURN 1; RETURN 2").unwrap();
    assert_eq!(plan(&query), Err(PlanError::MultipleStatementsUnsupported));
}
