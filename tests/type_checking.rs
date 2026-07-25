use cypher_rs::{
    analyze, analyze_with, infer_expression_type, infer_expression_type_with, parse, Clause,
    CypherType, Expr, FunctionSignature, Schema,
};

fn return_expr(source: &str) -> Expr {
    let query = parse(source).unwrap();
    let Clause::Return(ret) = query.clauses.last().unwrap() else {
        panic!("expected RETURN");
    };
    ret.items[0].expr.clone()
}

fn assert_type(source: &str, expected: CypherType) {
    assert_eq!(infer_expression_type(&return_expr(source)), expected);
}

#[test]
fn infers_literal_arithmetic_and_boolean_types() {
    assert_type("RETURN 1", CypherType::Integer);
    assert_type("RETURN 1.5", CypherType::Float);
    assert_type("RETURN 1 + 2.5", CypherType::Float);
    assert_type("RETURN 1 / 2", CypherType::Float);
    assert_type("RETURN 1 < 2 AND true", CypherType::Boolean);
    assert_type("RETURN 'a' + 'b'", CypherType::String);
}

#[test]
fn infers_collection_case_and_function_types() {
    assert_type(
        "RETURN [1, 2.5]",
        CypherType::List(Box::new(CypherType::Float)),
    );
    assert_type(
        "RETURN [x IN [1, 2] | x + 1]",
        CypherType::List(Box::new(CypherType::Any)),
    );
    assert_type(
        "RETURN CASE WHEN true THEN 1 ELSE 2.5 END",
        CypherType::Float,
    );
    assert_type("RETURN count(*)", CypherType::Integer);
    assert_type(
        "RETURN collect('x')",
        CypherType::List(Box::new(CypherType::String)),
    );
}

#[test]
fn unknown_runtime_values_remain_any() {
    assert_type("RETURN $value", CypherType::Any);
    assert_type("MATCH (n) RETURN n.value", CypherType::Any);
    assert_type("RETURN custom.function(1)", CypherType::Any);
}

#[test]
fn rejects_invalid_operator_operand_types() {
    for source in [
        "RETURN 'x' - 1",
        "RETURN 1 AND true",
        "RETURN 1 STARTS WITH 'x'",
        "RETURN 1 IN 2",
        "RETURN [1, 2][1.5]",
        "RETURN 1[0]",
        "RETURN 1[0..1]",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            report.errors().any(|issue| issue.code == "type-mismatch"),
            "expected type error for {source}, got {:?}",
            report.issues
        );
    }
}

#[test]
fn rejects_invalid_collection_expression_types() {
    for source in [
        "RETURN [x IN 1 | x]",
        "RETURN all(x IN [1] WHERE 1)",
        "RETURN filter(x IN 1 WHERE true)",
        "RETURN CASE WHEN 1 THEN true END",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(report.errors().any(|issue| issue.code == "type-mismatch"));
    }
}

#[test]
fn checks_clause_context_types() {
    for source in [
        "MATCH (n) WHERE 1 RETURN n",
        "RETURN 1 LIMIT 'one'",
        "UNWIND 1 AS x RETURN x",
        "FOREACH (x IN 1 | CREATE (n))",
        "LOAD CSV FROM 1 AS row RETURN row",
        "START n=node(*) WHERE 1 RETURN n",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            report.errors().any(|issue| issue.code == "type-mismatch"),
            "expected contextual type error for {source}, got {:?}",
            report.issues
        );
    }
}

#[test]
fn accepts_valid_typed_expressions_and_unknown_parameters() {
    for source in [
        "RETURN -1 + 2 * 3",
        "RETURN 'abc' STARTS WITH 'a'",
        "RETURN 1 IN [1, 2]",
        "RETURN [1, 2][0..1]",
        "MATCH (n) WHERE n.active RETURN n LIMIT $count",
        "UNWIND [1, 2] AS x RETURN x",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            !report.has_errors(),
            "unexpected errors for {source}: {:?}",
            report.issues
        );
    }
}

struct TypedSchema;

impl Schema for TypedSchema {
    fn variable_type(&self, variable: &str) -> Option<CypherType> {
        (variable == "n").then_some(CypherType::Node)
    }

    fn property_type(&self, variable: Option<&str>, property: &str) -> Option<CypherType> {
        match (variable, property) {
            (Some("n"), "age") => Some(CypherType::Integer),
            (Some("n"), "active") => Some(CypherType::Boolean),
            (Some("n"), "name") => Some(CypherType::String),
            _ => None,
        }
    }

    fn parameter_type(&self, parameter: &str) -> Option<CypherType> {
        match parameter {
            "count" => Some(CypherType::Integer),
            "bad_count" => Some(CypherType::String),
            _ => None,
        }
    }

    fn function_signature(&self, name: &str) -> Option<FunctionSignature> {
        match name {
            "app.score" => Some(FunctionSignature {
                arguments: vec![CypherType::String, CypherType::Integer],
                variadic: false,
                result: CypherType::Float,
            }),
            "app.concat" => Some(FunctionSignature {
                arguments: vec![CypherType::String, CypherType::String],
                variadic: true,
                result: CypherType::String,
            }),
            _ => None,
        }
    }

    fn function_signatures(&self, name: &str) -> Vec<FunctionSignature> {
        if name == "app.convert" {
            return vec![
                FunctionSignature {
                    arguments: vec![CypherType::Integer],
                    variadic: false,
                    result: CypherType::String,
                },
                FunctionSignature {
                    arguments: vec![CypherType::String],
                    variadic: false,
                    result: CypherType::Integer,
                },
            ];
        }
        self.function_signature(name).into_iter().collect()
    }
}

#[test]
fn schema_metadata_refines_inferred_types() {
    assert_eq!(
        infer_expression_type_with(&return_expr("MATCH (n) RETURN n"), &TypedSchema),
        CypherType::Node
    );
    assert_eq!(
        infer_expression_type_with(&return_expr("MATCH (n) RETURN n.age + 1"), &TypedSchema),
        CypherType::Integer
    );
    assert_eq!(
        infer_expression_type_with(&return_expr("RETURN app.score('x', 1)"), &TypedSchema),
        CypherType::Float
    );
    assert_eq!(
        infer_expression_type_with(&return_expr("RETURN $count"), &TypedSchema),
        CypherType::Integer
    );
}

#[test]
fn schema_types_participate_in_operator_and_context_checks() {
    for source in [
        "MATCH (n) RETURN n.name - 1",
        "MATCH (n) WHERE n.age RETURN n",
        "RETURN 1 LIMIT $bad_count",
    ] {
        let report = analyze_with(&parse(source).unwrap(), &TypedSchema);
        assert!(
            report.errors().any(|issue| issue.code == "type-mismatch"),
            "expected schema-aware type error for {source}: {:?}",
            report.issues
        );
    }
    assert!(!analyze_with(
        &parse("MATCH (n) WHERE n.active RETURN n LIMIT $count").unwrap(),
        &TypedSchema
    )
    .has_errors());
}

#[test]
fn validates_custom_function_signatures() {
    for source in [
        "RETURN app.score('x')",
        "RETURN app.score(1, 2)",
        "RETURN app.concat()",
        "RETURN app.concat('x', 'y', 3)",
    ] {
        let report = analyze_with(&parse(source).unwrap(), &TypedSchema);
        assert!(report.has_errors(), "expected function error for {source}");
    }
    assert!(!analyze_with(
        &parse("RETURN app.score('x', 1), app.concat('a', 'b', 'c')").unwrap(),
        &TypedSchema
    )
    .has_errors());
}

#[test]
fn validates_builtin_function_arity_and_collection_inputs() {
    for source in [
        "RETURN count()",
        "RETURN toString()",
        "RETURN size(1, 2)",
        "RETURN head(1)",
        "RETURN coalesce()",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            report
                .errors()
                .any(|issue| matches!(issue.code, "function-arity" | "type-mismatch")),
            "expected builtin function error for {source}: {:?}",
            report.issues
        );
    }
    for source in [
        "RETURN count(*)",
        "RETURN collect('x')",
        "RETURN head([1, 2])",
    ] {
        assert!(!analyze(&parse(source).unwrap()).has_errors(), "{source}");
    }
}

#[test]
fn validates_range_overloads() {
    for source in ["RETURN range(1, 5)", "RETURN range(1, 5, 2)"] {
        let query = parse(source).unwrap();
        assert!(!analyze(&query).has_errors(), "{source}");
        assert_eq!(
            infer_expression_type(&return_expr(source)),
            CypherType::List(Box::new(CypherType::Integer))
        );
    }
    for source in [
        "RETURN range(1)",
        "RETURN range(1, 2, 3, 4)",
        "RETURN range(1.5, 2)",
        "RETURN range(1, 'two')",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(report.has_errors(), "expected range error for {source}");
    }
}

#[test]
fn selects_schema_function_overloads() {
    assert_eq!(
        infer_expression_type_with(&return_expr("RETURN app.convert(1)"), &TypedSchema),
        CypherType::String
    );
    assert_eq!(
        infer_expression_type_with(&return_expr("RETURN app.convert('1')"), &TypedSchema),
        CypherType::Integer
    );
    for source in ["RETURN app.convert(true)", "RETURN app.convert(1, 2)"] {
        assert!(analyze_with(&parse(source).unwrap(), &TypedSchema).has_errors());
    }
}

#[test]
fn infers_standard_scalar_list_math_and_string_functions() {
    for (source, expected) in [
        ("RETURN abs(1)", CypherType::Integer),
        ("RETURN abs(1.5)", CypherType::Float),
        ("RETURN timestamp()", CypherType::Integer),
        ("RETURN sqrt(4)", CypherType::Float),
        ("RETURN substring('abc', 1)", CypherType::String),
        (
            "RETURN split('a,b', ',')",
            CypherType::List(Box::new(CypherType::String)),
        ),
        ("RETURN reverse('abc')", CypherType::String),
        ("RETURN toBoolean('true')", CypherType::Boolean),
    ] {
        assert_eq!(
            infer_expression_type(&return_expr(source)),
            expected,
            "{source}"
        );
        assert!(!analyze(&parse(source).unwrap()).has_errors(), "{source}");
    }

    let source = "MATCH p = (a)-[r]->(b) RETURN nodes(p), relationships(p), startNode(r)";
    assert!(!analyze(&parse(source).unwrap()).has_errors());
}

#[test]
fn rejects_invalid_standard_function_calls() {
    for source in [
        "RETURN abs('x')",
        "RETURN pi(1)",
        "RETURN substring(1, 0)",
        "RETURN replace('a', 'b')",
        "RETURN nodes(1)",
        "RETURN labels('x')",
        "RETURN atan2(1)",
        "RETURN percentileCont(1)",
        "RETURN sum('x')",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            report.has_errors(),
            "expected standard function error for {source}"
        );
    }
}

#[test]
fn infers_and_validates_spatial_values() {
    for (source, expected) in [
        ("RETURN point({x: 1, y: 2})", CypherType::Point),
        (
            "RETURN point({longitude: 1, latitude: 2}).x",
            CypherType::Float,
        ),
        ("RETURN point({x: 1, y: 2}).srid", CypherType::Integer),
        ("RETURN point({x: 1, y: 2}).crs", CypherType::String),
        (
            "RETURN distance(point({x: 1, y: 2}), point({x: 3, y: 4}))",
            CypherType::Float,
        ),
    ] {
        assert_eq!(
            infer_expression_type(&return_expr(source)),
            expected,
            "{source}"
        );
        assert!(!analyze(&parse(source).unwrap()).has_errors(), "{source}");
    }

    for source in [
        "RETURN point()",
        "RETURN point(1)",
        "RETURN distance(point({x: 1, y: 2}))",
        "RETURN distance(point({x: 1, y: 2}), {x: 3, y: 4})",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(report.has_errors(), "expected spatial error for {source}");
    }
}

#[test]
fn propagates_pattern_and_projection_alias_types() {
    for source in [
        "MATCH (n)-[r]->(m) RETURN n + 1",
        "MATCH (n)-[r]->(m) RETURN r + 1",
        "MATCH p = (n)-->(m) RETURN p + 1",
        "WITH 1 AS x RETURN x + 'bad'",
        "MATCH (n) WITH n AS alias RETURN alias + 1",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            report.errors().any(|issue| issue.code == "type-mismatch"),
            "expected propagated type error for {source}: {:?}",
            report.issues
        );
    }
}

#[test]
fn propagates_unwind_and_local_iterator_types() {
    for source in [
        "WITH [1, 2] AS xs UNWIND xs AS x RETURN x + 'bad'",
        "RETURN [x IN [1, 2] WHERE x + 'bad' > 0 | x]",
        "RETURN all(x IN [1, 2] WHERE x)",
        "RETURN reduce(acc = 0, x IN [1, 2] | acc + 'bad')",
        "FOREACH (x IN [1, 2] | CREATE (n {value: x + 'bad'}))",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            report.errors().any(|issue| issue.code == "type-mismatch"),
            "expected iterator type error for {source}: {:?}",
            report.issues
        );
    }

    for source in [
        "WITH [1, 2] AS xs UNWIND xs AS x RETURN x + 1",
        "RETURN [x IN [1, 2] WHERE x > 0 | x + 1]",
        "RETURN reduce(acc = 0, x IN [1, 2] | acc + x)",
        "FOREACH (x IN [1, 2] | CREATE (n {value: x + 1}))",
    ] {
        let report = analyze(&parse(source).unwrap());
        assert!(
            !report.has_errors(),
            "unexpected errors for {source}: {:?}",
            report.issues
        );
    }
}
