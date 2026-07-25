use std::collections::HashSet;

use cypher_rs::*;

#[test]
fn parses_load_csv_reference_form() {
    let query = parse("LOAD CSV FROM 'file:///movies.csv' AS row RETURN row").unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::LoadCsv(LoadCsvClause {
            with_headers: false,
            url: Expr::Literal(Literal::String(url)),
            variable,
            field_terminator: None,
        }) if url == "file:///movies.csv" && variable == "row"
    ));
}

#[test]
fn parses_headers_and_field_terminator() {
    let query = parse(
        "LOAD CSV WITH HEADERS FROM $source AS `record` FIELDTERMINATOR '\\t' RETURN `record`",
    )
    .unwrap();

    assert!(matches!(
        &query.clauses[0],
        Clause::LoadCsv(LoadCsvClause {
            with_headers: true,
            url: Expr::Param(source),
            variable,
            field_terminator: Some(terminator),
        }) if source == "source" && variable == "record" && terminator == "\t"
    ));
}

#[test]
fn url_can_be_an_expression_over_existing_bindings() {
    let query = parse("MATCH (n) LOAD CSV FROM n.url + '?raw=1' AS row RETURN row").unwrap();
    let Clause::LoadCsv(load_csv) = &query.clauses[1] else {
        panic!("expected LOAD CSV");
    };

    assert!(matches!(load_csv.url, Expr::Binary { op: BinOp::Add, .. }));
    assert!(!analyze(&query).has_errors());
}

#[test]
fn load_csv_binds_rows_but_not_its_own_url() {
    let query = parse("LOAD CSV FROM row AS row RETURN row").unwrap();
    let report = analyze(&query);

    assert!(report.bindings.contains("row"));
    assert!(report
        .errors()
        .any(|issue| issue.code == "unbound-variable" && issue.message.contains("`row`")));

    let query = parse("LOAD CSV FROM $source AS row RETURN row[0]").unwrap();
    assert!(!analyze(&query).has_errors());
}

#[test]
fn keywords_are_case_insensitive_and_have_boundaries() {
    let query =
        parse("load csv with headers from $source as row fieldterminator ',' return row").unwrap();
    assert!(matches!(query.clauses[0], Clause::LoadCsv(_)));

    assert!(parse("LOADCSV FROM $source AS row RETURN row").is_err());
    assert!(parse("LOAD CSV WITHHEADERS FROM $source AS row RETURN row").is_err());
    assert!(parse("LOAD CSV FROMAGE AS row RETURN row").is_err());
    assert!(parse("LOAD CSV FROM $source AS row FIELDTERMINATORed ',' RETURN row").is_err());
}

#[test]
fn rejects_malformed_load_csv_syntax() {
    for source in [
        "LOAD CSV",
        "LOAD CSV FROM AS row",
        "LOAD CSV FROM $source row",
        "LOAD CSV FROM $source AS",
        "LOAD CSV WITH FROM $source AS row",
        "LOAD CSV WITH HEADERS $source AS row",
        "LOAD CSV FROM $source AS row FIELDTERMINATOR",
        "LOAD CSV FROM $source AS row FIELDTERMINATOR 1",
    ] {
        assert!(parse(source).is_err(), "unexpectedly parsed {source}");
    }
}

#[test]
fn planner_preserves_load_csv_options() {
    let query = parse(
        "MATCH (n) LOAD CSV WITH HEADERS FROM n.url AS row FIELDTERMINATOR '|' RETURN n, row",
    )
    .unwrap();
    let plan = plan(&query).unwrap();
    let Plan::Project { input, .. } = &plan else {
        panic!("expected Project");
    };
    let Plan::LoadCsv {
        input,
        with_headers,
        url,
        variable,
        field_terminator,
    } = input.as_ref()
    else {
        panic!("expected LoadCsv");
    };

    assert!(*with_headers);
    assert!(matches!(url, Expr::Property { key, .. } if key == "url"));
    assert_eq!(variable, "row");
    assert_eq!(field_terminator.as_deref(), Some("|"));
    assert!(matches!(input.as_ref(), Plan::Scan { .. }));
}

#[test]
fn optimizer_pruning_cost_and_display_handle_load_csv() {
    let query =
        parse("MATCH (n) LOAD CSV FROM n.url AS row WITH * WHERE row[0] = 'Ada' RETURN n, row")
            .unwrap();
    let plan = plan(&query).unwrap();
    let optimized = optimize(plan.clone());
    let Plan::Project { input, .. } = &optimized else {
        panic!("expected optimized outer Project");
    };
    let Plan::Project { input, .. } = input.as_ref() else {
        panic!("expected optimized WITH Project");
    };
    assert!(matches!(input.as_ref(), Plan::Filter { input, .. }
        if matches!(input.as_ref(), Plan::LoadCsv { .. })));

    let Plan::Project { input, .. } = &plan else {
        panic!("expected outer Project");
    };
    let Plan::Filter { input, .. } = input.as_ref() else {
        panic!("expected Filter");
    };
    let Plan::Project { input, .. } = input.as_ref() else {
        panic!("expected WITH Project");
    };
    let load_csv = input.as_ref();

    assert_eq!(
        output_columns(load_csv),
        ["n".to_string(), "row".to_string()].into()
    );
    let demand = ["row".to_string()].into_iter().collect::<HashSet<_>>();
    assert_eq!(
        required_input_columns(load_csv, &demand),
        ["n".to_string()].into()
    );

    struct SevenRows;
    impl CostModel for SevenRows {
        fn scan_cardinality(&self, _label: Option<&str>) -> f64 {
            2.0
        }

        fn load_csv_rows(&self, _url: &Expr) -> f64 {
            7.0
        }
    }
    let estimate = estimate(load_csv, &SevenRows);
    assert_eq!(estimate.cardinality, 14.0);
    assert!(plan.to_string().contains("LoadCsv { with_headers: false"));
}
