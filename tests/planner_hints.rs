use std::collections::HashSet;

use cypher_rs::*;

fn first_match(input: &str) -> MatchClause {
    let query = parse(input).unwrap();
    match query.clauses.into_iter().next().expect("clause") {
        Clause::Match(match_clause) => match_clause,
        other => panic!("expected MATCH, got {other:?}"),
    }
}

#[test]
fn parses_index_scan_and_join_hints() {
    let index = first_match("MATCH (n:Person) USING INDEX n:Person(name) RETURN n");
    assert_eq!(
        index.hints,
        vec![MatchHint::Index {
            variable: "n".into(),
            label: "Person".into(),
            property: "name".into(),
        }]
    );

    let scan = first_match("MATCH (n:Person) USING SCAN n:Person RETURN n");
    assert_eq!(
        scan.hints,
        vec![MatchHint::Scan {
            variable: "n".into(),
            label: "Person".into(),
        }]
    );

    let join = first_match("MATCH (n), (m) USING JOIN ON n, m RETURN n");
    assert_eq!(
        join.hints,
        vec![MatchHint::Join {
            variables: vec!["n".into(), "m".into()],
        }]
    );
}

#[test]
fn accepts_multiple_hints_on_optional_matches() {
    let match_clause = first_match(
        "OPTIONAL MATCH (n:Person), (m:Movie) \
         USING INDEX n:Person(name) USING SCAN m:Movie USING JOIN ON n, m \
         RETURN n",
    );
    assert!(match_clause.optional);
    assert_eq!(match_clause.hints.len(), 3);
}

#[test]
fn supports_case_insensitive_keywords_and_escaped_names() {
    let match_clause = first_match(
        "MATCH (`person node`:`Person Label`) \
         using index `person node`:`Person Label`(`display name`) \
         RETURN `person node`",
    );
    assert!(matches!(
        &match_clause.hints[0],
        MatchHint::Index { variable, label, property }
            if variable == "person node"
                && label == "Person Label"
                && property == "display name"
    ));
    assert!(parse("MATCH (usingValue) RETURN usingValue").is_ok());
}

#[test]
fn rejects_malformed_or_detached_hints() {
    for input in [
        "USING SCAN n:Person MATCH (n) RETURN n",
        "MATCH (n) USING INDEX n:Person RETURN n",
        "MATCH (n) USING INDEX n:Person() RETURN n",
        "MATCH (n) USING SCAN n RETURN n",
        "MATCH (n) USING JOIN ON RETURN n",
        "MATCH (n) WHERE n.ok USING SCAN n:Person RETURN n",
    ] {
        assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
    }
}

struct PersonSchema;

impl Schema for PersonSchema {
    fn has_label(&self, label: &str) -> bool {
        label == "Person"
    }
}

#[test]
fn semantic_analysis_checks_hint_bindings_and_labels() {
    let query =
        parse("MATCH (n:Person) USING INDEX missing:Unknown(name) USING JOIN ON n, other RETURN n")
            .unwrap();
    let report = analyze_with(&query, &PersonSchema);
    assert!(report
        .errors()
        .any(|issue| issue.code == "unknown-label" && issue.message.contains("Unknown")));
    assert!(report
        .errors()
        .any(|issue| issue.code == "unbound-variable" && issue.message.contains("missing")));
    assert!(report
        .errors()
        .any(|issue| issue.code == "unbound-variable" && issue.message.contains("other")));
}

#[test]
fn planner_and_optimizer_preserve_hints() {
    let query = parse("MATCH (n:Person) USING INDEX n:Person(name) WHERE n.ok RETURN n").unwrap();
    let planned = plan(&query).unwrap();
    assert!(planned.to_string().contains("PlannerHint"));
    assert!(planned.to_string().contains("Index"));
    assert!(format!("{}", optimize(planned)).contains("PlannerHint"));
}

#[test]
fn hints_are_transparent_to_columns_and_cost() {
    let plain = plan(&parse("MATCH (n:Person) RETURN n").unwrap()).unwrap();
    let hinted = plan(&parse("MATCH (n:Person) USING SCAN n:Person RETURN n").unwrap()).unwrap();

    assert_eq!(output_columns(&hinted), ["n".to_string()].into());
    assert_eq!(
        required_input_columns(&hinted, &HashSet::new()),
        ["n".to_string()].into()
    );
    let model = CardinalityCostModel::default().with_label("Person", 100.0);
    assert_eq!(estimate(&plain, &model), estimate(&hinted, &model));
}
