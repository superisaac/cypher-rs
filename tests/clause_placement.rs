use cypher_rs::*;

#[test]
fn accepts_match_and_with_predicates_in_their_valid_positions() {
    assert!(parse("MATCH (n) WHERE n.ok WITH n WHERE n.active RETURN n").is_ok());
    assert!(parse("MATCH (n) WITH n ORDER BY n.name SKIP 1 LIMIT 2 WHERE n.ok RETURN n").is_ok());
}

#[test]
fn rejects_detached_and_repeated_where_clauses() {
    for input in [
        "WHERE true RETURN 1",
        "MATCH (n) WHERE n.ok WHERE n.active RETURN n",
        "RETURN 1 WHERE true",
        "MATCH (n) ORDER BY n.name WHERE n.ok RETURN n",
    ] {
        assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
    }
}

#[test]
fn accepts_projection_modifiers_in_reference_order() {
    for input in [
        "RETURN 1 ORDER BY 1 SKIP 2 LIMIT 3",
        "RETURN 1 SKIP 2 LIMIT 3",
        "RETURN 1 LIMIT 3",
        "WITH 1 AS n ORDER BY n LIMIT 1 RETURN n",
    ] {
        assert!(parse(input).is_ok(), "failed to parse {input:?}");
    }
}

#[test]
fn rejects_detached_projection_modifiers() {
    for input in [
        "ORDER BY 1 RETURN 1",
        "SKIP 1 RETURN 1",
        "LIMIT 1 RETURN 1",
        "MATCH (n) ORDER BY n.name RETURN n",
        "MATCH (n) LIMIT 1 RETURN n",
    ] {
        assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
    }
}

#[test]
fn rejects_duplicate_or_misordered_projection_modifiers() {
    for input in [
        "RETURN 1 ORDER BY 1 ORDER BY 1",
        "RETURN 1 SKIP 1 ORDER BY 1",
        "RETURN 1 LIMIT 1 SKIP 1",
        "RETURN 1 LIMIT 1 LIMIT 2",
        "WITH 1 AS n WHERE true LIMIT 1 RETURN n",
    ] {
        assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
    }
}

#[test]
fn return_terminates_each_union_branch() {
    for input in [
        "RETURN 1 MATCH (n)",
        "RETURN 1 RETURN 2",
        "MATCH (n) RETURN n CREATE (m)",
    ] {
        assert!(parse(input).is_err(), "unexpectedly parsed {input:?}");
    }

    assert!(parse("RETURN 1 UNION MATCH (n) RETURN n").is_ok());
}

#[test]
fn validates_every_statement_independently() {
    assert!(parse("RETURN 1 ORDER BY 1; MATCH (n) WHERE n.ok RETURN n;").is_ok());
    assert!(parse("RETURN 1; LIMIT 2 RETURN 2").is_err());
}
