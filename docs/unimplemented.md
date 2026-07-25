# Unimplemented openCypher Syntax

This document tracks syntax that is not yet implemented by `cypher-rs`.
It is based on the current `src/cypher.pest` grammar and uses
`vendors/libcypher-parser` as the primary reference. Some items exposed by
`libcypher-parser`, such as `START` and `CREATE UNIQUE`, are legacy syntax
rather than current core openCypher features.

## Missing Clauses and Statements

No known clause or statement syntax remains unimplemented in the tracked
reference scope.

## Partially Implemented Syntax

No known partially implemented syntax remains in the tracked reference scope.
Runtime-dependent values continue to infer as `ANY` unless a `Schema` supplies
more precise metadata.

## Parsed but Not Planned

The parser and AST support the following operations, but the logical planner
returns `PlanError::UnsupportedClause` for them:

- `DELETE` and `DETACH DELETE`
- `UNWIND`
- `FOREACH`
- `START`
- `CREATE INDEX`
- `DROP INDEX`
- `CREATE CONSTRAINT`
- `DROP CONSTRAINT`

## Suggested Implementation Order

1. [x] Function calls and aggregates (completed 2026-07-25; parser, AST,
   semantic traversal, logical-plan expression preservation, and tests).
2. [x] `CASE` expressions (completed 2026-07-25; searched and simple forms,
   optional `ELSE`, semantic traversal, logical-plan expression preservation,
   and tests).
3. [x] `UNION` and `UNION ALL` (completed 2026-07-25; parser, AST,
   branch-scoped semantic analysis, logical plan, optimizer, pruning, cost,
   projection-count validation, and tests).
4. [x] Variable-length relationships and shortest paths (completed 2026-07-25;
   all range forms, `shortestPath` / `allShortestPaths` pattern wrappers,
   semantic validation, logical plan, optimizer, pruning, cost, and tests).
5. [x] List comprehensions and collection predicates (completed 2026-07-25;
   optional filtering and projection, `all` / `any` / `none` / `single`,
   local iterator scope, planner preservation, optimizer, pruning, and tests).
6. [x] `CALL ... YIELD` (completed 2026-07-25; namespaced procedures,
   optional arguments, yielded fields and aliases, attached `WHERE`, semantic
   bindings, logical plan, optimizer, pruning, cost, display, and tests).
7. [x] Subscripts, slices, and map projections (completed 2026-07-25;
   dynamic and chained subscripts, all slice-bound forms, all map projection
   selector forms, postfix chaining, semantic traversal, optimizer, pruning,
   logical-plan preservation, and tests).
8. [x] Escaped identifiers and complete literal escaping (completed
   2026-07-25; backtick-delimited symbolic names with embedded backticks,
   consistent decoding across grammar positions, control/quote/backslash
   string escapes, Unicode scalar and surrogate-pair escapes, validation,
   semantic/planning preservation, and tests).
9. [x] Pattern comprehensions (completed 2026-07-25; optional path variables
   and predicates, required projections, variable-length patterns, escaped
   names, local pattern scope, nested schema validation, logical-plan
   preservation, optimizer, pruning, and tests).
10. [x] `FILTER`, `EXTRACT`, and `REDUCE` expressions (completed 2026-07-25;
    optional predicate/projection/reduction forms, nested expressions, escaped
    local names, accumulator and iterator scope, logical-plan preservation,
    optimizer, pruning, and tests).
11. [x] Pattern expressions (completed 2026-07-25; bare and `EXISTS`
    arguments, shortest-path forms, named-variable binding and schema
    validation, logical-plan preservation, optimizer dependency tracking,
    pruning, and tests).
12. [x] Label predicates (completed 2026-07-25; single and multiple labels,
    escaped names, postfix precedence, binding and schema validation,
    logical-plan preservation, optimizer dependency tracking, pruning, and
    tests).
13. [x] Additional expression operators (completed 2026-07-25; `XOR`,
    regular-expression matching (`=~`), right-associative exponentiation (`^`),
    unary plus, reference-compatible precedence, semantic traversal,
    logical-plan preservation, optimizer, pruning, and tests).
14. [x] General chained comparisons (completed 2026-07-25; same and mixed
    `<`, `>`, `<=`, and `>=` chains, explicit chain AST preserving shared
    operands, arithmetic precedence, single-comparison AST compatibility,
    semantic traversal, logical-plan preservation, optimizer, pruning, and
    tests).
15. [x] Named paths (completed 2026-07-25; regular and shortest-path forms,
    escaped path variables, semantic bindings and schema validation,
    `NamedPath` logical-plan output, filter-pushdown boundaries, optimizer,
    pruning, cost, display, and tests).
16. [x] Parameters as complete pattern property maps (completed 2026-07-25;
    node and relationship forms, escaped parameter names, anonymous-element
    bindings, semantic and schema traversal, `PropertyMapFilter` logical-plan
    preservation, static-map compatibility, optimizer, pruning, cost,
    display, and tests).
17. [x] Full Unicode symbolic-name support (completed 2026-07-25; Unicode
    `ID_Start` / `ID_Continue`, connector-punctuation starts,
    currency-symbol continuations, combining marks, Unicode-aware keyword
    boundaries, all identifier-bearing grammar positions, semantic/planning
    preservation, and tests).
18. [x] Scientific-notation floating-point literals (completed 2026-07-25;
    integer, decimal, and leading-dot mantissas, signed exponents, unary-sign
    precedence, malformed and non-finite value rejection, logical-plan and
    cost preservation, and tests).
19. [x] Hexadecimal and octal integer literals (completed 2026-07-25; `0x`
    hexadecimal and legacy leading-zero octal forms, shared radix parsing for
    expressions and relationship ranges, unary-sign precedence, invalid digit
    and overflow rejection, logical-plan and cost preservation, and tests).
20. [x] Block comments (completed 2026-07-25; `/* ... */` comments between
    tokens and operators, multiline and trailing forms, literal and escaped
    identifier isolation, unterminated-comment rejection, compatibility with
    line comments, semantic/planning transparency, and tests).
21. [x] Trailing semicolons and multiple statements (completed 2026-07-25;
    optional trailing delimiters, explicit AST statement boundaries, comments
    and whitespace around delimiters, statement-scoped semantic analysis,
    explicit single-plan API rejection for batches, MCP statement and clause
    counts, malformed empty-statement rejection, and tests).
22. [x] `RETURN` and `WITH` wildcard projections (completed 2026-07-25;
    standalone and mixed leading `*`, `DISTINCT` forms, explicit AST and plan
    preservation flags, `WITH *` binding propagation, optimizer and projection
    analysis support, expanded UNION column-count validation, malformed
    wildcard placement rejection, display, and tests).
23. [x] Clause placement constraints (completed 2026-07-25; `WHERE` attachment
    to `MATCH` and `WITH`, projection-scoped `ORDER BY` / `SKIP` / `LIMIT`,
    reference-compatible modifier ordering, duplicate and detached modifier
    rejection, terminal `RETURN` enforcement, per-UNION-branch and
    per-statement validation, and tests).
24. [x] MATCH planner hints (completed 2026-07-25; `USING INDEX`, `USING SCAN`,
    and `USING JOIN ON`, multiple hints, optional matches, escaped names,
    structured AST representation, binding and schema validation, transparent
    logical-plan nodes, optimizer, projection analysis, cost, display,
    contextual keyword compatibility, malformed syntax rejection, and tests).
25. [x] Complete parameter syntax (completed 2026-07-25; named and positional
    `$name` / `$1` forms, legacy `{name}` / `{1}` forms, escaped and Unicode
    names, shared `Expr::Param` representation, map-literal disambiguation,
    complete node and relationship property maps, invalid zero-prefixed and
    incomplete parameter rejection, semantic/planning preservation, and tests).
26. [x] `FOREACH` (completed 2026-07-25; parser, recursive AST, multi-clause
    and nested bodies, local iterator scope, semantic traversal, explicit
    read-only planner rejection, malformed syntax rejection, and tests).
27. [x] `LOAD CSV` (completed 2026-07-25; optional `WITH HEADERS`, expression
    URLs, escaped row bindings, optional decoded `FIELDTERMINATOR`, semantic
    scope, logical plan, filter-pushdown boundary, projection analysis, cost
    model extension, display, malformed syntax rejection, and tests).
28. [x] `USING PERIODIC COMMIT` (completed 2026-07-25; structured per-statement
    query-option AST, optional radix-aware integer limits, multiple-statement
    preservation, logical-plan wrapper, optimizer/projection/cost transparency,
    display, MCP option counts, malformed placement rejection, and tests).
29. [x] `CREATE INDEX` (completed 2026-07-25; single and composite property
    indexes, escaped names, schema-command AST and statement isolation,
    comments and case-insensitive keywords, semantic acceptance, explicit
    read-only planner rejection, malformed syntax rejection, and tests).
30. [x] `DROP INDEX` (completed 2026-07-25; single and composite property
    indexes, shared index-command parser, escaped names, comments and
    case-insensitive keywords, schema-statement isolation, semantic acceptance,
    explicit read-only planner rejection, malformed syntax rejection, and tests).
31. [x] `CREATE CONSTRAINT` (completed 2026-07-25; unique node constraints,
    node property-existence constraints, relationship property-existence
    constraints in all directions, escaped names, local expression scope,
    schema-statement isolation, explicit read-only planner rejection, malformed
    syntax rejection, and tests).
32. [x] `DROP CONSTRAINT` (completed 2026-07-25; unique node constraints,
    node and relationship property-existence constraints in all directions,
    shared create/drop constraint parser, escaped names, local expression
    scope, schema-statement isolation, explicit read-only planner rejection,
    malformed syntax rejection, and tests).
33. [x] `EXPLAIN` (completed 2026-07-25; structured statement-option AST,
    query and schema-command prefixes, ordering with query hints,
    per-statement preservation, logical-plan wrapper,
    optimizer/projection/cost transparency, display, malformed placement
    rejection, and tests).
34. [x] `PROFILE` (completed 2026-07-25; structured statement-option AST,
    query and schema-command prefixes, ordering with query hints,
    per-statement preservation, logical-plan wrapper,
    optimizer/projection/cost transparency, display, malformed placement
    rejection, and tests).
35. [x] `CYPHER <version>` and Cypher options (completed 2026-07-25;
    integer and dotted versions, structured key/value settings, combined
    version and settings forms, composition with statement and query options,
    schema-command prefixes, per-statement preservation, logical-plan
    transparency, malformed syntax rejection, and tests).
36. [x] `START` (completed 2026-07-25; node and relationship ID lookups,
    all-entity scans, index property lookups and query forms, multiple start
    points, attached predicates, escaped names and parameters, semantic
    bindings, first-clause placement validation, explicit read-only planner
    rejection, malformed syntax rejection, and tests).
37. [x] `CREATE UNIQUE` (completed 2026-07-25; reference-compatible unique
    flag on the CREATE AST, regular and multiple patterns, properties and
    escaped names, FOREACH bodies, semantic traversal, explicit read-only
    planner rejection, malformed syntax rejection, and tests).
38. [x] Core expression type inference and checking (completed 2026-07-25;
    public `CypherType` and `infer_expression_type` API, all expression
    variants, numeric promotion and collection/CASE/function result types,
    arithmetic, boolean, string, comparison, membership, subscript and slice
    validation, collection predicates, WHERE/LIMIT/SKIP/UNWIND/FOREACH/LOAD
    CSV/START contexts, conservative `ANY` handling, diagnostics, and tests).
39. [x] Schema-driven expression types (completed 2026-07-25; backward-
    compatible `Schema` hooks for variables, properties, parameters, and
    custom function signatures, public schema-aware inference API, exact
    operator and clause-context validation, function result types, fixed and
    variadic argument count/type checking, diagnostics, and tests).
40. [x] Common built-in function signatures (completed 2026-07-25; `count`,
    `collect`, scalar conversions, predicates, collection accessors, numeric
    aggregates, `coalesce`, fixed and variadic arity checks, collection input
    validation, wildcard handling, diagnostics, and tests).
41. [x] Query-local type propagation (completed 2026-07-25; sequential clause
    type environments, node, relationship and path bindings, `WITH` aliases,
    `UNWIND` elements, CSV rows, procedure yields, list comprehensions,
    collection predicates, FILTER/EXTRACT/REDUCE locals, pattern
    comprehensions, nested FOREACH bodies, diagnostics, and tests).
42. [x] Function overload resolution and `range` signatures (completed
    2026-07-25; backward-compatible schema overload sets, arity candidate
    selection, argument-type matching, result-type selection and unification,
    two- and three-argument integer `range` forms, diagnostics, and tests).
43. [x] Standard non-spatial function signatures (completed 2026-07-25;
    scalar conversions, graph entity and path accessors, list and string
    functions, numeric aggregates, statistics, arithmetic, logarithmic and
    trigonometric functions, overloaded result inference, arity and argument
    validation, diagnostics, and tests).
44. [x] Spatial expression types and functions (completed 2026-07-25; public
    `POINT` type, `point(MAP)`, `distance(POINT, POINT)`, coordinate, SRID and
    CRS property inference, overload-based arity and argument validation,
    diagnostics, and tests).
45. [x] Logical planning for `CREATE` and `CREATE UNIQUE` (completed
    2026-07-25; pattern-preserving create plan node, UNIQUE flag, input and
    output binding propagation, property dependency analysis, optimizer
    transparency, cost model, display, and tests).
46. [x] Logical planning for `MERGE` (completed 2026-07-25; pattern and
    conditional action preservation, input and output binding propagation,
    pattern and update-expression dependency analysis, optimizer transparency,
    cost model, display, and tests).
47. [x] Logical planning for `SET` (completed 2026-07-25; preservation of
    property replacement, property merging, nested property assignment and
    label updates, input dependency analysis, output-column transparency,
    optimizer traversal, cost model, display, and tests).
48. [x] Logical planning for `REMOVE` (completed 2026-07-25; property and
    multi-label removal preservation, target dependency analysis,
    output-column transparency, optimizer traversal, cost model, display, and
    tests).

This order prioritizes syntax commonly found in real queries and the
openCypher Technology Compatibility Kit over legacy extensions.
