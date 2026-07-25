# Unimplemented openCypher Syntax

This document tracks syntax that is not yet implemented by `cypher-rs`.
It is based on the current `src/cypher.pest` grammar and uses
`vendors/libcypher-parser` as the primary reference. Some items exposed by
`libcypher-parser`, such as `START` and `CREATE UNIQUE`, are legacy syntax
rather than current core openCypher features.

## Missing Clauses and Statements

| Area | Unimplemented syntax |
|---|---|
| Query composition | `UNION`, `UNION ALL` |
| Procedures | `CALL ... YIELD ... WHERE` |
| Iteration and import | `FOREACH`, `LOAD CSV`, `USING PERIODIC COMMIT` |
| Planner hints | `USING INDEX`, `USING SCAN`, `USING JOIN ON` |
| Schema commands | `CREATE INDEX`, `DROP INDEX`, `CREATE CONSTRAINT`, `DROP CONSTRAINT` |
| Query options | `EXPLAIN`, `PROFILE`, `CYPHER <version>` and Cypher options |
| Legacy clauses | `START`, `CREATE UNIQUE` |

## Missing Expressions

- Function calls and aggregate functions, including `count(*)`, `sum(n.x)`,
  and `coalesce(...)`.
- `CASE WHEN ... THEN ... ELSE ... END` and simple `CASE` expressions.
- List comprehensions such as `[x IN xs WHERE x.active | x.name]`.
- Pattern comprehensions.
- `FILTER`, `EXTRACT`, and `REDUCE` expressions.
- `all`, `any`, `none`, and `single` predicates.
- Pattern expressions, including expressions used by `EXISTS`.
- Collection subscripts such as `xs[0]`.
- Collection slices such as `xs[1..3]`, `xs[..3]`, and `xs[1..]`.
- Map projections such as `n { .name, age: n.age, .* }`.
- Label predicates such as `n:Person`.
- `XOR`, regular-expression matching (`=~`), exponentiation (`^`), and unary
  plus (`+value`).
- General chained comparisons.

## Missing Pattern Syntax

- Named paths such as `p = (a)-[:KNOWS]->(b)`.
- `shortestPath(...)` and `allShortestPaths(...)`.
- Variable-length relationships such as `-[r:KNOWS*1..5]->`.
- Pattern comprehensions and pattern expressions.
- Parameters as complete node or relationship property maps where supported
  by the reference grammar.

## Missing Lexical Syntax

- Escaped symbolic names such as `` `user name` ``.
- Full Unicode symbolic-name support.
- Complete openCypher string escape handling.
- Scientific-notation floating-point literals.
- Hexadecimal and octal integer literals.
- Block comments (`/* ... */`).
- Trailing statement semicolons and multiple statements per input.

## Partially Implemented Syntax

- `RETURN` and `WITH` do not support `*`, including mixed projections such as
  `RETURN *, n.name`.
- `WHERE`, `ORDER BY`, `SKIP`, and `LIMIT` are represented as independent AST
  clauses. This accepts common query forms but does not enforce all placement
  constraints from the full openCypher grammar.
- `MATCH` and `OPTIONAL MATCH` support basic patterns but not planner hints,
  named paths, shortest paths, or variable-length relationships.
- Property access supports `.name`, but not subscripts, slices, label
  predicates, or map projections.
- Numeric literals are limited to unsigned decimal integers and simple decimal
  floats. Negative values are represented through unary negation.
- Parameters use the `$name` form only.
- Semantic analysis checks bindings, scope, labels, and relationship types,
  but does not perform complete expression type inference or type checking.

## Parsed but Not Planned

The parser and AST support the following update clauses, but the read-only
logical planner returns `PlanError::UnsupportedClause` for them:

- `CREATE`
- `MERGE`
- `SET`
- `REMOVE`
- `DELETE` and `DETACH DELETE`
- `UNWIND`

## Suggested Implementation Order

1. Function calls and aggregates.
2. `CASE` expressions.
3. `UNION` and `UNION ALL`.
4. Variable-length relationships and shortest paths.
5. List comprehensions and collection predicates.
6. `CALL ... YIELD`.
7. Subscripts, slices, and map projections.
8. Escaped identifiers and complete literal escaping.

This order prioritizes syntax commonly found in real queries and the
openCypher Technology Compatibility Kit over legacy extensions.
