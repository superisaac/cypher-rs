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
fn escaped_names_work_across_patterns_properties_and_aliases() {
    let query = parse(
        "MATCH (`user node`:`User Label`)-[`relationship id`:`HAS TYPE`]->(`return`) \
         RETURN `user node`.`display name` AS `result value`",
    )
    .unwrap();
    let Clause::Match(match_clause) = &query.clauses[0] else {
        panic!("expected MATCH");
    };
    let pattern = &match_clause.patterns[0];
    assert_eq!(pattern.anchor.var.as_deref(), Some("user node"));
    assert_eq!(pattern.anchor.labels, ["User Label"]);
    assert_eq!(pattern.chain[0].rel.var.as_deref(), Some("relationship id"));
    assert_eq!(pattern.chain[0].rel.types, ["HAS TYPE"]);
    assert_eq!(pattern.chain[0].node.var.as_deref(), Some("return"));

    let Clause::Return(return_clause) = &query.clauses[1] else {
        panic!("expected RETURN");
    };
    assert_eq!(
        return_clause.items[0].alias.as_deref(),
        Some("result value")
    );
    assert!(matches!(
        &return_clause.items[0].expr,
        Expr::Property { base, key }
            if key == "display name"
                && matches!(base.as_ref(), Expr::Variable(name) if name == "user node")
    ));
}

#[test]
fn escaped_names_decode_embedded_backticks_and_keywords() {
    let expression = first_return_expr("RETURN `odd``name` AS `match`");
    assert_eq!(expression, Expr::Variable("odd`name".into()));

    assert!(parse("RETURN `unterminated").is_err());
    assert!(parse("RETURN odd`name").is_err());
}

#[test]
fn escaped_names_work_in_maps_parameters_and_local_variables() {
    let expression = first_return_expr(
        "RETURN {`display key`: $`user id`, projected: [`item value` IN $rows | `item value`]} ",
    );
    let Expr::Map(entries) = expression else {
        panic!("expected map");
    };
    assert_eq!(entries[0].0, "display key");
    assert_eq!(entries[0].1, Expr::Param("user id".into()));
    assert!(matches!(
        &entries[1].1,
        Expr::ListComprehension {
            variable,
            projection: Some(projection),
            ..
        } if variable == "item value"
            && matches!(projection.as_ref(), Expr::Variable(name) if name == "item value")
    ));
}

#[test]
fn escaped_names_work_in_function_and_procedure_names() {
    let expression = first_return_expr("RETURN `my funcs`.`coalesce value`(1)");
    assert!(matches!(
        expression,
        Expr::FunctionCall { name, .. } if name == "my funcs.coalesce value"
    ));

    let query = parse(
        "CALL `db tools`.`find users`($id) YIELD `user node` AS `result user` \
         RETURN `result user`",
    )
    .unwrap();
    assert!(matches!(
        &query.clauses[0],
        Clause::Call(call)
            if call.name == "db tools.find users"
                && call.yields[0].field == "user node"
                && call.yields[0].binding() == "result user"
    ));
    assert!(!analyze(&query).has_errors());
}

#[test]
fn decodes_control_quote_and_backslash_escapes() {
    let expression = first_return_expr(r#"RETURN '\a\b\f\n\r\t\v\\\'\"\?'"#);
    assert_eq!(
        expression,
        Expr::Literal(Literal::String(
            "\u{0007}\u{0008}\u{000c}\n\r\t\u{000b}\\'\"?".into()
        ))
    );

    let expression = first_return_expr("RETURN 'first\nsecond'");
    assert_eq!(
        expression,
        Expr::Literal(Literal::String("first\nsecond".into()))
    );
}

#[test]
fn decodes_unicode_escapes_and_surrogate_pairs() {
    let expression = first_return_expr(r"RETURN '\u0041\u03A9\U0001F600'");
    assert_eq!(expression, Expr::Literal(Literal::String("AΩ😀".into())));

    let expression = first_return_expr(r"RETURN '\uD83D\uDE00'");
    assert_eq!(expression, Expr::Literal(Literal::String("😀".into())));
}

#[test]
fn rejects_unknown_incomplete_and_invalid_unicode_escapes() {
    for query in [
        r"RETURN '\x'",
        r"RETURN '\u123'",
        r"RETURN '\U00110000'",
        r"RETURN '\uD83D'",
        r"RETURN '\uD83D\u0041'",
        r"RETURN '\uDE00'",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn decoded_identifiers_flow_through_sema_planning_and_pruning() {
    let query =
        parse("MATCH (`user node`) RETURN `user node`.`display name` AS `name value`").unwrap();
    assert!(!analyze(&query).has_errors());
    let plan = plan(&query).unwrap();
    let demand = ["name value".to_string()]
        .into_iter()
        .collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(&plan, &demand),
        ["user node".to_string()].into()
    );
}
