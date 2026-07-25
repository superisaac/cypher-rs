use cypher_rs::*;

#[test]
fn parses_create_patterns() {
    let q = parse("CREATE (a:Person {name: 'Ada'}), (b)-[:KNOWS]->(c)").unwrap();
    match &q.clauses[0] {
        Clause::Create(create) => {
            assert_eq!(create.patterns.len(), 2);
            assert_eq!(create.patterns[0].anchor.var.as_deref(), Some("a"));
            assert_eq!(create.patterns[0].anchor.labels, ["Person"]);
            assert_eq!(create.patterns[1].chain.len(), 1);
        }
        other => panic!("expected CREATE, got {other:?}"),
    }
}

#[test]
fn parses_merge_with_actions() {
    let q = parse(
        "MERGE (n:Person {id: $id}) \
         ON CREATE SET n.created = true, n += $defaults \
         ON MATCH SET n.seen = true",
    )
    .unwrap();
    match &q.clauses[0] {
        Clause::Merge(merge) => {
            assert_eq!(merge.pattern.anchor.var.as_deref(), Some("n"));
            assert_eq!(merge.actions.len(), 2);
            assert_eq!(merge.actions[0].kind, MergeActionKind::OnCreate);
            assert_eq!(merge.actions[0].items.len(), 2);
            assert!(matches!(
                merge.actions[0].items[1],
                SetItem::MergeProperties { .. }
            ));
            assert_eq!(merge.actions[1].kind, MergeActionKind::OnMatch);
        }
        other => panic!("expected MERGE, got {other:?}"),
    }
}

#[test]
fn parses_all_set_item_forms() {
    let q = parse(
        "MATCH (n) SET n.name = 'Ada', n.profile.rank = 1, \
         n = $replacement, n += {active: true}, n:Person:Active RETURN n",
    )
    .unwrap();
    match &q.clauses[1] {
        Clause::Set(set) => {
            assert_eq!(set.items.len(), 5);
            assert!(matches!(set.items[0], SetItem::Property { .. }));
            match &set.items[1] {
                SetItem::Property {
                    property: Expr::Property { base, key },
                    ..
                } => {
                    assert_eq!(key, "rank");
                    assert!(
                        matches!(base.as_ref(), Expr::Property { key, .. } if key == "profile")
                    );
                }
                other => panic!("expected nested property assignment, got {other:?}"),
            }
            assert!(matches!(set.items[2], SetItem::AllProperties { .. }));
            assert!(matches!(set.items[3], SetItem::MergeProperties { .. }));
            assert!(matches!(
                &set.items[4],
                SetItem::Labels { labels, .. } if labels == &["Person", "Active"]
            ));
        }
        other => panic!("expected SET, got {other:?}"),
    }
}

#[test]
fn parses_delete_and_detach_delete() {
    let q = parse("MATCH (n)-[r]-() DELETE r, n").unwrap();
    assert!(matches!(
        &q.clauses[1],
        Clause::Delete(DeleteClause { detach: false, expressions }) if expressions.len() == 2
    ));

    let q = parse("MATCH (n) DETACH DELETE n").unwrap();
    assert!(matches!(
        &q.clauses[1],
        Clause::Delete(DeleteClause { detach: true, expressions }) if expressions.len() == 1
    ));
}

#[test]
fn parses_unwind() {
    let q = parse("UNWIND [1, 2, 3] AS value RETURN value").unwrap();
    match &q.clauses[0] {
        Clause::Unwind(unwind) => {
            assert_eq!(unwind.alias, "value");
            assert!(matches!(&unwind.expr, Expr::List(items) if items.len() == 3));
        }
        other => panic!("expected UNWIND, got {other:?}"),
    }
    assert!(!analyze(&q).has_errors());
}

#[test]
fn keywords_are_case_insensitive_and_have_boundaries() {
    assert!(matches!(
        parse("create (n)").unwrap().clauses[0],
        Clause::Create(_)
    ));
    assert!(matches!(
        parse("unwind $items as item return item").unwrap().clauses[0],
        Clause::Unwind(_)
    ));
    assert!(parse("CREATEd (n)").is_err());
    assert!(parse("DETACHDELETE n").is_err());
}

#[test]
fn rejects_invalid_update_syntax() {
    assert!(parse("CREATE").is_err());
    assert!(parse("MERGE (n), (m)").is_err());
    assert!(parse("SET n").is_err());
    assert!(parse("DELETE").is_err());
    assert!(parse("UNWIND [1, 2] value").is_err());
}

#[test]
fn planner_reports_update_clauses_as_unsupported() {
    let q = parse("CREATE (n)").unwrap();
    assert_eq!(plan(&q), Err(PlanError::UnsupportedClause("CREATE")));
}
