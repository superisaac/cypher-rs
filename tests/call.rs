use std::collections::HashSet;

use cypher_rs::*;

#[test]
fn parses_namespaced_calls_with_optional_arguments() {
    let query = parse("CALL db. labels").unwrap();
    assert!(matches!(
        &query.clauses[0],
        Clause::Call(CallClause {
            name,
            arguments,
            yields,
            predicate: None,
        }) if name == "db.labels" && arguments.is_empty() && yields.is_empty()
    ));

    let query = parse("CALL example.math.proc(1 + n, 'value', $arg)").unwrap();
    assert!(matches!(
        &query.clauses[0],
        Clause::Call(CallClause { name, arguments, .. })
            if name == "example.math.proc"
                && arguments.len() == 3
                && matches!(arguments[0], Expr::Binary { op: BinOp::Add, .. })
    ));
}

#[test]
fn parses_yield_aliases_and_attached_where() {
    let query =
        parse("CALL db.search($term) YIELD node AS result, score WHERE score >= 0.5 RETURN result")
            .unwrap();
    match &query.clauses[0] {
        Clause::Call(call) => {
            assert_eq!(call.yields.len(), 2);
            assert_eq!(call.yields[0].field, "node");
            assert_eq!(call.yields[0].alias.as_deref(), Some("result"));
            assert_eq!(call.yields[0].binding(), "result");
            assert_eq!(call.yields[1].binding(), "score");
            assert!(matches!(
                call.predicate,
                Some(Expr::Binary { op: BinOp::Gte, .. })
            ));
        }
        other => panic!("expected CALL, got {other:?}"),
    }
    assert!(matches!(query.clauses[1], Clause::Return(_)));
}

#[test]
fn parses_blank_yield_projection() {
    let query = parse("CALL db.await() YIELD -").unwrap();
    assert!(matches!(
        &query.clauses[0],
        Clause::Call(CallClause { yields, .. }) if yields.is_empty()
    ));
}

#[test]
fn call_keywords_are_case_insensitive_and_have_boundaries() {
    let query = parse("call db.labels() yield label as name return name").unwrap();
    assert!(matches!(query.clauses[0], Clause::Call(_)));
    assert!(parse("CALLed db.labels()").is_err());
    assert!(parse("CALL db.labels() YIELDed label").is_err());
}

#[test]
fn rejects_malformed_calls() {
    for query in [
        "CALL",
        "CALL ()",
        "CALL db.proc(",
        "CALL db.proc(,)",
        "CALL db.proc() YIELD",
        "CALL db.proc() YIELD value AS",
        "CALL db.proc() WHERE",
    ] {
        assert!(parse(query).is_err(), "unexpectedly parsed {query}");
    }
}

#[test]
fn semantic_analysis_checks_arguments_and_yield_predicate() {
    let query = parse(
        "MATCH (n) CALL db.proc(n, missing) YIELD value AS result \
         WHERE result > absent RETURN result",
    )
    .unwrap();
    let report = analyze(&query);
    let messages = report
        .errors()
        .map(|issue| issue.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message.contains("`missing`")));
    assert!(messages.iter().any(|message| message.contains("`absent`")));
    assert!(!messages.iter().any(|message| message.contains("`result`")));
    assert!(report.bindings.contains("result"));
}

#[test]
fn planner_preserves_call_and_filters_yielded_rows() {
    let query =
        parse("MATCH (n) CALL db.score(n) YIELD score WHERE score > 0 RETURN n, score").unwrap();
    let plan = plan(&query).unwrap();
    let Plan::Project { input, .. } = &plan else {
        panic!("expected Project, got {plan:?}");
    };
    let Plan::Filter { input, .. } = input.as_ref() else {
        panic!("expected Filter below Project");
    };
    let Plan::ProcedureCall {
        input,
        name,
        arguments,
        yields,
    } = input.as_ref()
    else {
        panic!("expected ProcedureCall below Filter");
    };
    assert_eq!(name, "db.score");
    assert_eq!(arguments, &[Expr::Variable("n".into())]);
    assert_eq!(yields[0].binding(), "score");
    assert!(matches!(input.as_ref(), Plan::Scan { .. }));
}

#[test]
fn optimizer_pruning_cost_and_display_handle_calls() {
    let query =
        parse("MATCH (n) CALL db.score(n) YIELD score WHERE score > 0 RETURN score").unwrap();
    let plan = plan(&query).unwrap();
    let optimized = optimize(plan.clone());
    let Plan::Project { input, .. } = &optimized else {
        panic!("expected Project");
    };
    assert!(matches!(input.as_ref(), Plan::Filter { input, .. }
        if matches!(input.as_ref(), Plan::ProcedureCall { .. })));

    let Plan::Project { input, .. } = &plan else {
        unreachable!();
    };
    let Plan::Filter { input, .. } = input.as_ref() else {
        unreachable!();
    };
    let procedure = input.as_ref();
    assert_eq!(
        output_columns(procedure),
        ["n".to_string(), "score".to_string()].into()
    );
    let demand = ["score".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(procedure, &demand),
        ["n".to_string()].into()
    );

    let estimate = estimate(&plan, &CardinalityCostModel::default());
    assert!(estimate.cardinality > 0.0);
    assert!(estimate.cost > 0.0);
    assert!(plan.to_string().contains("ProcedureCall { name: db.score"));
}
