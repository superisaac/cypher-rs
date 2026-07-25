use cypher_rs::{parse, plan};

#[test]
fn every_parsed_clause_family_has_a_planning_path() {
    let cases = [
        (
            "read pipeline",
            "MATCH (n) WHERE n.active WITH n RETURN n ORDER BY n.name SKIP 1 LIMIT 2",
        ),
        ("create", "CREATE (n)"),
        ("create unique", "CREATE UNIQUE (n)"),
        (
            "merge",
            "MERGE (n {id: $id}) ON CREATE SET n.created = true",
        ),
        ("set", "MATCH (n) SET n.active = true"),
        ("remove", "MATCH (n) REMOVE n.old"),
        ("delete", "MATCH (n) DELETE n"),
        ("detach delete", "MATCH (n) DETACH DELETE n"),
        ("unwind", "UNWIND [1, 2] AS value RETURN value"),
        (
            "foreach",
            "FOREACH (value IN [1, 2] | CREATE (n {value: value}))",
        ),
        ("start", "START n=node(*) RETURN n"),
        (
            "load csv",
            "LOAD CSV FROM 'file:///rows.csv' AS row RETURN row",
        ),
        ("call", "CALL db.labels() YIELD label RETURN label"),
        ("union", "RETURN 1 AS value UNION ALL RETURN 2 AS value"),
    ];

    for (name, source) in cases {
        let query = parse(source).unwrap_or_else(|error| panic!("{name} did not parse: {error}"));
        plan(&query).unwrap_or_else(|error| panic!("{name} did not plan: {error}"));
    }
}

#[test]
fn every_schema_command_variant_has_a_planning_path() {
    let cases = [
        ("create index", "CREATE INDEX ON :Person(name)"),
        ("drop index", "DROP INDEX ON :Person(name)"),
        (
            "create node constraint",
            "CREATE CONSTRAINT ON (n:Person) ASSERT n.id IS UNIQUE",
        ),
        (
            "create relationship constraint",
            "CREATE CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since)",
        ),
        (
            "drop node constraint",
            "DROP CONSTRAINT ON (n:Person) ASSERT n.id IS UNIQUE",
        ),
        (
            "drop relationship constraint",
            "DROP CONSTRAINT ON ()-[r:KNOWS]-() ASSERT exists(r.since)",
        ),
    ];

    for (name, source) in cases {
        let query = parse(source).unwrap_or_else(|error| panic!("{name} did not parse: {error}"));
        plan(&query).unwrap_or_else(|error| panic!("{name} did not plan: {error}"));
    }
}

#[test]
fn every_query_option_has_a_planning_path() {
    let cases = [
        (
            "explain, cypher, and periodic commit",
            "EXPLAIN CYPHER 3.5 runtime=slotted USING PERIODIC COMMIT 10 \
             LOAD CSV FROM 'file:///rows.csv' AS row RETURN row",
        ),
        ("profile", "PROFILE RETURN 1"),
    ];

    for (name, source) in cases {
        let query = parse(source).unwrap_or_else(|error| panic!("{name} did not parse: {error}"));
        plan(&query).unwrap_or_else(|error| panic!("{name} did not plan: {error}"));
    }
}
