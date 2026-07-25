use cypher_rs::*;

fn return_exprs(input: &str) -> Vec<Expr> {
    let query = parse(input).unwrap();
    let Clause::Return(return_clause) = &query.clauses[0] else {
        panic!("expected RETURN clause");
    };
    return_clause
        .items
        .iter()
        .map(|item| item.expr.clone())
        .collect()
}

#[test]
fn parses_named_and_positional_dollar_parameters() {
    assert_eq!(
        return_exprs("RETURN $name, $1, $42"),
        vec![
            Expr::Param("name".into()),
            Expr::Param("1".into()),
            Expr::Param("42".into()),
        ]
    );
}

#[test]
fn parses_legacy_named_and_positional_parameters() {
    assert_eq!(
        return_exprs("RETURN {name}, {1}, {42}"),
        vec![
            Expr::Param("name".into()),
            Expr::Param("1".into()),
            Expr::Param("42".into()),
        ]
    );
}

#[test]
fn supports_escaped_and_unicode_parameter_names_in_both_forms() {
    assert_eq!(
        return_exprs("RETURN $`user id`, {`legacy name`}, $参数, {参数}"),
        vec![
            Expr::Param("user id".into()),
            Expr::Param("legacy name".into()),
            Expr::Param("参数".into()),
            Expr::Param("参数".into()),
        ]
    );
}

#[test]
fn distinguishes_legacy_parameters_from_map_literals() {
    let expressions = return_exprs("RETURN {name}, {}, {name: $value}, {nested: {legacy}}");
    assert!(matches!(&expressions[0], Expr::Param(name) if name == "name"));
    assert!(matches!(&expressions[1], Expr::Map(entries) if entries.is_empty()));
    assert!(matches!(&expressions[2], Expr::Map(entries)
        if entries == &vec![("name".into(), Expr::Param("value".into()))]));
    assert!(matches!(&expressions[3], Expr::Map(entries)
        if entries == &vec![("nested".into(), Expr::Param("legacy".into()))]));
}

#[test]
fn supports_all_parameter_forms_as_complete_pattern_property_maps() {
    for input in [
        "MATCH (n $props) RETURN n",
        "MATCH (n $1) RETURN n",
        "MATCH (n {props}) RETURN n",
        "MATCH (n {1}) RETURN n",
        "MATCH (n)-[r $2]->(m) RETURN r",
        "MATCH (n)-[r {rels}]->(m) RETURN r",
    ] {
        assert!(parse(input).is_ok(), "failed to parse {input:?}");
    }
}

#[test]
fn rejects_invalid_parameter_names_and_incomplete_forms() {
    for input in [
        "RETURN $",
        "RETURN {} AS parameter",
        "RETURN $0",
        "RETURN {0}",
        "RETURN $01",
        "RETURN {01}",
        "RETURN $1name",
        "RETURN {1name}",
        "RETURN {name",
    ] {
        if input == "RETURN {} AS parameter" {
            assert!(matches!(return_exprs(input)[0], Expr::Map(_)));
        } else {
            assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
        }
    }
}

#[test]
fn semantic_and_plan_paths_treat_every_form_as_external() {
    let query = parse("RETURN $1, {legacy}, $modern").unwrap();
    assert!(!analyze(&query).has_errors());
    let planned = plan(&query).unwrap();
    assert!(matches!(planned, Plan::Project { ref exprs, .. }
        if exprs.iter().all(|item| matches!(item.expr, Expr::Param(_)))));
    assert!(matches!(optimize(planned), Plan::Project { .. }));
}
