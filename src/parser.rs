use pest::iterators::Pair;
use pest::Parser as _;
use pest_derive::Parser;

use crate::ast::*;
use crate::error::ParseError;

#[derive(Parser)]
#[grammar = "cypher.pest"]
pub(crate) struct CypherParser;

/// Parse an openCypher query string into a [`Query`] AST.
///
/// See the crate root for supported features and limitations.
pub fn parse(input: &str) -> Result<Query, ParseError> {
    let mut pairs = CypherParser::parse(Rule::query, input)?;
    let query_pair = pairs
        .next()
        .ok_or_else(|| ParseError::Unexpected("empty parse".into()))?;

    let mut statements = Vec::new();
    for inner in query_pair.into_inner() {
        match inner.as_rule() {
            Rule::EOI => continue,
            Rule::statement => statements.push(walk_statement(inner)?),
            r => return Err(unexpected("clause", r)),
        }
    }
    let mut statements = statements.into_iter();
    let first = statements
        .next()
        .ok_or_else(|| ParseError::Unexpected("query: missing statement".into()))?;
    let (additional_statement_options, additional_statements) = statements
        .map(|statement| (statement.options, statement.clauses))
        .unzip();
    Ok(Query {
        options: first.options,
        clauses: first.clauses,
        additional_statements,
        additional_statement_options,
    })
}

struct ParsedStatement {
    options: Vec<QueryOption>,
    clauses: Vec<Clause>,
}

fn walk_statement(pair: Pair<Rule>) -> Result<ParsedStatement, ParseError> {
    let mut options = Vec::new();
    let mut clauses = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::explain_option => options.push(QueryOption::Explain),
            Rule::profile_option => options.push(QueryOption::Profile),
            Rule::cypher_option => options.push(walk_cypher_option(inner)?),
            Rule::query_option => options.push(walk_query_option(inner)?),
            Rule::create_index_command | Rule::drop_index_command => {
                clauses.push(Clause::SchemaCommand(walk_index_command(inner)?));
            }
            Rule::create_constraint_command | Rule::drop_constraint_command => {
                clauses.push(Clause::SchemaCommand(walk_constraint_command(inner)?));
            }
            _ => clauses.push(walk_clause(inner)?),
        }
    }
    if matches!(clauses.as_slice(), [Clause::SchemaCommand(_)]) {
        return Ok(ParsedStatement { options, clauses });
    }
    validate_clause_placement(&clauses)?;
    Ok(ParsedStatement { options, clauses })
}

fn walk_cypher_option(pair: Pair<Rule>) -> Result<QueryOption, ParseError> {
    let mut version = None;
    let mut settings = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::cypher_version => version = Some(inner.as_str().to_owned()),
            Rule::cypher_setting => {
                let mut names = inner.into_inner().filter(|p| p.as_rule() == Rule::ident);
                let key = names.next().map(ident_name).unwrap_or_default();
                let value = names.next().map(ident_name).unwrap_or_default();
                settings.push((key, value));
            }
            _ => {}
        }
    }
    Ok(QueryOption::Cypher { version, settings })
}

fn walk_query_option(pair: Pair<Rule>) -> Result<QueryOption, ParseError> {
    let limit = pair
        .into_inner()
        .find(|inner| inner.as_rule() == Rule::integer)
        .map(|inner| parse_unsigned_integer(inner.as_str()))
        .transpose()?;
    Ok(QueryOption::UsingPeriodicCommit { limit })
}

fn walk_index_command(pair: Pair<Rule>) -> Result<SchemaCommand, ParseError> {
    let rule = pair.as_rule();
    let names = pair
        .into_inner()
        .filter(|inner| inner.as_rule() == Rule::ident)
        .map(ident_name)
        .collect::<Vec<_>>();
    let (label, properties) = names
        .split_first()
        .ok_or_else(|| ParseError::Unexpected("index command: missing label".into()))?;
    let label = label.clone();
    let properties = properties.to_vec();
    match rule {
        Rule::create_index_command => Ok(SchemaCommand::CreateIndex { label, properties }),
        Rule::drop_index_command => Ok(SchemaCommand::DropIndex { label, properties }),
        other => Err(unexpected("index command", other)),
    }
}

enum ConstraintTarget {
    Node {
        variable: String,
        label: String,
    },
    Relationship {
        variable: String,
        relationship_type: String,
    },
}

fn walk_constraint_command(pair: Pair<Rule>) -> Result<SchemaCommand, ParseError> {
    let rule = pair.as_rule();
    let mut target = None;
    let mut expression = None;
    let mut unique = false;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::node_constraint_target => {
                let mut names = inner.into_inner().map(ident_name);
                target = Some(ConstraintTarget::Node {
                    variable: names.next().ok_or_else(|| {
                        ParseError::Unexpected("constraint: missing node variable".into())
                    })?,
                    label: names.next().ok_or_else(|| {
                        ParseError::Unexpected("constraint: missing node label".into())
                    })?,
                });
            }
            Rule::relationship_constraint_target => {
                let mut names = inner.into_inner().map(ident_name);
                target = Some(ConstraintTarget::Relationship {
                    variable: names.next().ok_or_else(|| {
                        ParseError::Unexpected("constraint: missing relationship variable".into())
                    })?,
                    relationship_type: names.next().ok_or_else(|| {
                        ParseError::Unexpected("constraint: missing relationship type".into())
                    })?,
                });
            }
            Rule::expr => expression = Some(walk_expr(inner)?),
            Rule::unique_constraint => unique = true,
            _ => {}
        }
    }

    let expression = expression
        .ok_or_else(|| ParseError::Unexpected("constraint: missing expression".into()))?;
    match (
        rule,
        target.ok_or_else(|| ParseError::Unexpected("constraint: missing target".into()))?,
    ) {
        (Rule::create_constraint_command, ConstraintTarget::Node { variable, label }) => {
            Ok(SchemaCommand::CreateNodeConstraint {
                variable,
                label,
                expression,
                unique,
            })
        }
        (
            Rule::create_constraint_command,
            ConstraintTarget::Relationship {
                variable,
                relationship_type,
            },
        ) => Ok(SchemaCommand::CreateRelationshipConstraint {
            variable,
            relationship_type,
            expression,
        }),
        (Rule::drop_constraint_command, ConstraintTarget::Node { variable, label }) => {
            Ok(SchemaCommand::DropNodeConstraint {
                variable,
                label,
                expression,
                unique,
            })
        }
        (
            Rule::drop_constraint_command,
            ConstraintTarget::Relationship {
                variable,
                relationship_type,
            },
        ) => Ok(SchemaCommand::DropRelationshipConstraint {
            variable,
            relationship_type,
            expression,
        }),
        (other, _) => Err(unexpected("constraint command", other)),
    }
}

fn walk_clause(pair: Pair<Rule>) -> Result<Clause, ParseError> {
    match pair.as_rule() {
        Rule::match_clause => Ok(Clause::Match(walk_match(pair)?)),
        Rule::create_clause => Ok(Clause::Create(walk_create(pair)?)),
        Rule::merge_clause => Ok(Clause::Merge(walk_merge(pair)?)),
        Rule::set_clause => Ok(Clause::Set(walk_set(pair)?)),
        Rule::remove_clause => Ok(Clause::Remove(walk_remove(pair)?)),
        Rule::delete_clause => Ok(Clause::Delete(walk_delete(pair)?)),
        Rule::unwind_clause => Ok(Clause::Unwind(walk_unwind(pair)?)),
        Rule::foreach_clause => Ok(Clause::Foreach(walk_foreach(pair)?)),
        Rule::start_clause => Ok(Clause::Start(walk_start(pair)?)),
        Rule::load_csv_clause => Ok(Clause::LoadCsv(walk_load_csv(pair)?)),
        Rule::call_clause => Ok(Clause::Call(walk_call(pair)?)),
        Rule::where_clause => Ok(Clause::Where(walk_clause_expr(pair)?)),
        Rule::with_clause => Ok(Clause::With(walk_return(pair)?)),
        Rule::return_clause => Ok(Clause::Return(walk_return(pair)?)),
        Rule::order_by_clause => Ok(Clause::OrderBy(walk_order_by(pair)?)),
        Rule::limit_clause => Ok(Clause::Limit(walk_clause_expr(pair)?)),
        Rule::skip_clause => Ok(Clause::Skip(walk_clause_expr(pair)?)),
        Rule::union_clause => Ok(Clause::Union(UnionClause {
            all: pair.into_inner().any(|item| item.as_rule() == Rule::kw_all),
        })),
        r => Err(unexpected("clause", r)),
    }
}

fn walk_start(pair: Pair<Rule>) -> Result<StartClause, ParseError> {
    let mut points = Vec::new();
    let mut predicate = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::start_point => points.push(walk_start_point(inner)?),
            Rule::expr => predicate = Some(walk_expr(inner)?),
            _ => {}
        }
    }
    Ok(StartClause { points, predicate })
}

fn walk_start_point(pair: Pair<Rule>) -> Result<StartPoint, ParseError> {
    let mut parts = pair.into_inner();
    let variable = ident_name(
        parts
            .next()
            .ok_or_else(|| ParseError::Unexpected("START: missing variable".into()))?,
    );
    let entity = match parts
        .next()
        .and_then(|p| p.into_inner().next())
        .map(|p| p.as_rule())
    {
        Some(Rule::kw_node) => StartEntity::Node,
        Some(Rule::kw_rel | Rule::kw_relationship) => StartEntity::Relationship,
        _ => return Err(ParseError::Unexpected("START: invalid entity kind".into())),
    };
    let remaining = parts.collect::<Vec<_>>();
    let lookup = match remaining.first().map(|p| p.as_rule()) {
        None => StartLookup::All,
        Some(Rule::integer) => StartLookup::Ids(
            remaining
                .iter()
                .map(|p| {
                    parse_unsigned_integer(p.as_str()).and_then(|n| {
                        i64::try_from(n).map_err(|_| ParseError::InvalidInt(p.as_str().into()))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(Rule::ident) => {
            let name = ident_name(remaining[0].clone());
            match remaining.len() {
                2 => StartLookup::Index {
                    name,
                    property: None,
                    value: walk_start_value(remaining[1].clone())?,
                },
                3 => StartLookup::Index {
                    name,
                    property: Some(ident_name(remaining[1].clone())),
                    value: walk_start_value(remaining[2].clone())?,
                },
                _ => {
                    return Err(ParseError::Unexpected(
                        "START: malformed index lookup".into(),
                    ))
                }
            }
        }
        _ => return Err(ParseError::Unexpected("START: malformed lookup".into())),
    };
    Ok(StartPoint {
        variable,
        entity,
        lookup,
    })
}

fn walk_start_value(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let inner = if pair.as_rule() == Rule::start_lookup_value {
        first_inner(pair, "START lookup value")?
    } else {
        pair
    };
    walk_expr(inner)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectionOwner {
    With,
    Return,
}

struct ProjectionModifiers {
    owner: ProjectionOwner,
    order_by: bool,
    skip: bool,
    limit: bool,
    where_clause: bool,
}

impl ProjectionModifiers {
    fn new(owner: ProjectionOwner) -> Self {
        Self {
            owner,
            order_by: false,
            skip: false,
            limit: false,
            where_clause: false,
        }
    }
}

fn validate_clause_placement(clauses: &[Clause]) -> Result<(), ParseError> {
    let mut branch_start = 0;
    for (index, clause) in clauses.iter().enumerate() {
        if matches!(clause, Clause::Union(_)) {
            validate_branch_placement(&clauses[branch_start..index])?;
            branch_start = index + 1;
        }
    }
    validate_branch_placement(&clauses[branch_start..])
}

fn validate_branch_placement(clauses: &[Clause]) -> Result<(), ParseError> {
    if clauses
        .iter()
        .enumerate()
        .any(|(index, clause)| matches!(clause, Clause::Start(_)) && index != 0)
    {
        return Err(placement_error("START must be the first clause"));
    }
    let mut match_where_available = false;
    let mut projection = None::<ProjectionModifiers>;

    for clause in clauses {
        match clause {
            Clause::Where(_) => {
                if match_where_available {
                    match_where_available = false;
                    continue;
                }
                let Some(state) = projection.as_mut() else {
                    return Err(placement_error("WHERE must follow MATCH or WITH"));
                };
                if state.owner != ProjectionOwner::With || state.where_clause {
                    return Err(placement_error("WHERE must follow MATCH or WITH"));
                }
                state.where_clause = true;
            }
            Clause::OrderBy(_) => {
                let Some(state) = projection.as_mut() else {
                    return Err(placement_error("ORDER BY must follow WITH or RETURN"));
                };
                if state.order_by || state.skip || state.limit || state.where_clause {
                    return Err(placement_error(
                        "ORDER BY must precede SKIP, LIMIT, and WHERE",
                    ));
                }
                state.order_by = true;
            }
            Clause::Skip(_) => {
                let Some(state) = projection.as_mut() else {
                    return Err(placement_error("SKIP must follow WITH or RETURN"));
                };
                if state.skip || state.limit || state.where_clause {
                    return Err(placement_error("SKIP must precede LIMIT and WHERE"));
                }
                state.skip = true;
            }
            Clause::Limit(_) => {
                let Some(state) = projection.as_mut() else {
                    return Err(placement_error("LIMIT must follow WITH or RETURN"));
                };
                if state.limit || state.where_clause {
                    return Err(placement_error("LIMIT must precede WHERE"));
                }
                state.limit = true;
            }
            Clause::Union(_) => unreachable!("UNION branches are validated separately"),
            clause => {
                if projection
                    .as_ref()
                    .is_some_and(|state| state.owner == ProjectionOwner::Return)
                {
                    return Err(placement_error("RETURN must terminate a UNION branch"));
                }

                match_where_available = false;
                projection = None;
                match clause {
                    Clause::Match(_) => match_where_available = true,
                    Clause::With(_) => {
                        projection = Some(ProjectionModifiers::new(ProjectionOwner::With));
                    }
                    Clause::Return(_) => {
                        projection = Some(ProjectionModifiers::new(ProjectionOwner::Return));
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn placement_error(message: &str) -> ParseError {
    ParseError::Unexpected(format!("invalid clause placement: {message}"))
}

// --- clauses --------------------------------------------------------------

fn walk_clause_expr(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let inner = first_operand(pair, "clause expr")?;
    walk_expr(inner)
}

fn walk_create(pair: Pair<Rule>) -> Result<CreateClause, ParseError> {
    let mut unique = false;
    let pattern_list = pair
        .into_inner()
        .inspect(|p| unique |= p.as_rule() == Rule::kw_unique)
        .find(|p| p.as_rule() == Rule::pattern_list)
        .ok_or_else(|| ParseError::Unexpected("create: missing pattern list".into()))?;
    let patterns = pattern_list
        .into_inner()
        .map(walk_pattern)
        .collect::<Result<_, _>>()?;
    Ok(CreateClause { unique, patterns })
}

fn walk_merge(pair: Pair<Rule>) -> Result<MergeClause, ParseError> {
    let mut pattern = None;
    let mut actions = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::pattern => pattern = Some(walk_pattern(inner)?),
            Rule::merge_action => actions.push(walk_merge_action(inner)?),
            _ => {}
        }
    }
    Ok(MergeClause {
        pattern: pattern.ok_or_else(|| ParseError::Unexpected("merge: missing pattern".into()))?,
        actions,
    })
}

fn walk_merge_action(pair: Pair<Rule>) -> Result<MergeAction, ParseError> {
    let mut kind = None;
    let mut items = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::merge_action_kind => {
                kind = Some(if inner.as_str().eq_ignore_ascii_case("MATCH") {
                    MergeActionKind::OnMatch
                } else {
                    MergeActionKind::OnCreate
                });
            }
            Rule::set_items => items = Some(walk_set_items(inner)?),
            _ => {}
        }
    }
    Ok(MergeAction {
        kind: kind.ok_or_else(|| ParseError::Unexpected("merge action: missing kind".into()))?,
        items: items.ok_or_else(|| ParseError::Unexpected("merge action: missing SET".into()))?,
    })
}

fn walk_set(pair: Pair<Rule>) -> Result<SetClause, ParseError> {
    let items = pair
        .into_inner()
        .find(|p| p.as_rule() == Rule::set_items)
        .ok_or_else(|| ParseError::Unexpected("set: missing items".into()))?;
    Ok(SetClause {
        items: walk_set_items(items)?,
    })
}

fn walk_set_items(pair: Pair<Rule>) -> Result<Vec<SetItem>, ParseError> {
    pair.into_inner().map(walk_set_item).collect()
}

fn walk_set_item(pair: Pair<Rule>) -> Result<SetItem, ParseError> {
    let item = first_inner(pair, "set item")?;
    let rule = item.as_rule();
    let mut inner = item.into_inner();
    match rule {
        Rule::set_property => {
            let property =
                walk_property_target(inner.next().ok_or_else(|| {
                    ParseError::Unexpected("set property: missing target".into())
                })?)?;
            let value =
                walk_expr(inner.next().ok_or_else(|| {
                    ParseError::Unexpected("set property: missing value".into())
                })?)?;
            Ok(SetItem::Property { property, value })
        }
        Rule::set_all_properties | Rule::merge_properties => {
            let variable = ident_name(
                inner
                    .next()
                    .ok_or_else(|| ParseError::Unexpected("set map: missing variable".into()))?,
            );
            let value = walk_expr(
                inner
                    .next()
                    .ok_or_else(|| ParseError::Unexpected("set map: missing value".into()))?,
            )?;
            if rule == Rule::set_all_properties {
                Ok(SetItem::AllProperties { variable, value })
            } else {
                Ok(SetItem::MergeProperties { variable, value })
            }
        }
        Rule::set_labels => {
            let variable =
                ident_name(inner.next().ok_or_else(|| {
                    ParseError::Unexpected("set labels: missing variable".into())
                })?);
            let labels = inner
                .map(|label| first_inner(label, "SET label").map(ident_name))
                .collect::<Result<Vec<_>, ParseError>>()?;
            Ok(SetItem::Labels { variable, labels })
        }
        r => Err(unexpected("set item", r)),
    }
}

fn walk_property_target(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut inner = pair.into_inner();
    let var = inner
        .next()
        .ok_or_else(|| ParseError::Unexpected("property target: missing variable".into()))?;
    let mut expr = walk_expr(var)?;
    for access in inner {
        let key = first_inner(access, "property target access")?;
        expr = Expr::Property {
            base: Box::new(expr),
            key: ident_name(key),
        };
    }
    Ok(expr)
}

fn walk_remove(pair: Pair<Rule>) -> Result<RemoveClause, ParseError> {
    let items = pair
        .into_inner()
        .find(|p| p.as_rule() == Rule::remove_items)
        .ok_or_else(|| ParseError::Unexpected("remove: missing items".into()))?;
    Ok(RemoveClause {
        items: items
            .into_inner()
            .map(walk_remove_item)
            .collect::<Result<_, _>>()?,
    })
}

fn walk_remove_item(pair: Pair<Rule>) -> Result<RemoveItem, ParseError> {
    let item = first_inner(pair, "remove item")?;
    match item.as_rule() {
        Rule::property_target => Ok(RemoveItem::Property(walk_property_target(item)?)),
        Rule::remove_labels => {
            let mut inner = item.into_inner();
            let variable =
                ident_name(inner.next().ok_or_else(|| {
                    ParseError::Unexpected("remove labels: missing variable".into())
                })?);
            let labels = inner
                .map(|label| first_inner(label, "REMOVE label").map(ident_name))
                .collect::<Result<Vec<_>, ParseError>>()?;
            Ok(RemoveItem::Labels { variable, labels })
        }
        r => Err(unexpected("remove item", r)),
    }
}

fn walk_delete(pair: Pair<Rule>) -> Result<DeleteClause, ParseError> {
    let mut detach = false;
    let mut expressions = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::kw_detach => detach = true,
            Rule::expression_list => {
                expressions = inner
                    .into_inner()
                    .map(walk_expr)
                    .collect::<Result<_, _>>()?;
            }
            _ => {}
        }
    }
    Ok(DeleteClause {
        detach,
        expressions,
    })
}

fn walk_unwind(pair: Pair<Rule>) -> Result<UnwindClause, ParseError> {
    let mut expr = None;
    let mut alias = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => expr = Some(walk_expr(inner)?),
            Rule::ident => alias = Some(ident_name(inner)),
            _ => {}
        }
    }
    Ok(UnwindClause {
        expr: expr.ok_or_else(|| ParseError::Unexpected("unwind: missing expression".into()))?,
        alias: alias.ok_or_else(|| ParseError::Unexpected("unwind: missing alias".into()))?,
    })
}

fn walk_foreach(pair: Pair<Rule>) -> Result<ForeachClause, ParseError> {
    let mut variable = None;
    let mut expression = None;
    let mut clauses = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident if variable.is_none() => variable = Some(ident_name(inner)),
            Rule::expr => expression = Some(walk_expr(inner)?),
            Rule::match_clause
            | Rule::create_clause
            | Rule::merge_clause
            | Rule::set_clause
            | Rule::remove_clause
            | Rule::delete_clause
            | Rule::unwind_clause
            | Rule::foreach_clause
            | Rule::start_clause
            | Rule::load_csv_clause
            | Rule::call_clause
            | Rule::where_clause
            | Rule::with_clause
            | Rule::return_clause
            | Rule::order_by_clause
            | Rule::limit_clause
            | Rule::skip_clause => clauses.push(walk_clause(inner)?),
            _ => {}
        }
    }

    Ok(ForeachClause {
        variable: variable
            .ok_or_else(|| ParseError::Unexpected("foreach: missing variable".into()))?,
        expression: expression
            .ok_or_else(|| ParseError::Unexpected("foreach: missing expression".into()))?,
        clauses,
    })
}

fn walk_load_csv(pair: Pair<Rule>) -> Result<LoadCsvClause, ParseError> {
    let mut with_headers = false;
    let mut url = None;
    let mut variable = None;
    let mut field_terminator = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::with_headers => with_headers = true,
            Rule::expr => url = Some(walk_expr(inner)?),
            Rule::ident => variable = Some(ident_name(inner)),
            Rule::field_terminator => {
                let literal = inner
                    .into_inner()
                    .find(|item| item.as_rule() == Rule::string_lit)
                    .ok_or_else(|| {
                        ParseError::Unexpected("load csv: missing field terminator".into())
                    })?;
                field_terminator = Some(decode_string_literal(literal.as_str())?);
            }
            _ => {}
        }
    }

    Ok(LoadCsvClause {
        with_headers,
        url: url.ok_or_else(|| ParseError::Unexpected("load csv: missing URL".into()))?,
        variable: variable
            .ok_or_else(|| ParseError::Unexpected("load csv: missing variable".into()))?,
        field_terminator,
    })
}

fn walk_call(pair: Pair<Rule>) -> Result<CallClause, ParseError> {
    let mut name = None;
    let mut arguments = Vec::new();
    let mut yields = Vec::new();
    let mut predicate = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::kw_call | Rule::kw_yield | Rule::kw_where => {}
            Rule::procedure_name => {
                name = Some(
                    inner
                        .into_inner()
                        .map(ident_name)
                        .collect::<Vec<_>>()
                        .join("."),
                );
            }
            Rule::call_arguments => {
                if let Some(list) = inner.into_inner().next() {
                    arguments = list
                        .into_inner()
                        .map(walk_expr)
                        .collect::<Result<Vec<_>, _>>()?;
                }
            }
            Rule::yield_body => {
                if let Some(body) = inner.into_inner().next() {
                    if body.as_rule() == Rule::yield_items {
                        yields = body
                            .into_inner()
                            .map(walk_yield_item)
                            .collect::<Result<Vec<_>, _>>()?;
                    }
                }
            }
            Rule::expr => predicate = Some(walk_expr(inner)?),
            r => return Err(unexpected("CALL", r)),
        }
    }

    Ok(CallClause {
        name: name.ok_or_else(|| ParseError::Unexpected("CALL: missing procedure name".into()))?,
        arguments,
        yields,
        predicate,
    })
}

fn walk_yield_item(pair: Pair<Rule>) -> Result<YieldItem, ParseError> {
    let mut identifiers = pair
        .into_inner()
        .filter(|item| item.as_rule() == Rule::ident)
        .map(ident_name);
    let field = identifiers
        .next()
        .ok_or_else(|| ParseError::Unexpected("YIELD: missing field".into()))?;
    Ok(YieldItem {
        field,
        alias: identifiers.next(),
    })
}

fn walk_match(pair: Pair<Rule>) -> Result<MatchClause, ParseError> {
    let mut patterns = Vec::new();
    let mut hints = Vec::new();
    let mut optional = false;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::optional_kw => optional = true,
            Rule::pattern_list => {
                for p in inner.into_inner() {
                    patterns.push(walk_pattern(p)?);
                }
            }
            Rule::using_index_hint | Rule::using_scan_hint | Rule::using_join_hint => {
                hints.push(walk_match_hint(inner)?);
            }
            _ => {}
        }
    }
    Ok(MatchClause {
        optional,
        patterns,
        hints,
    })
}

fn walk_match_hint(pair: Pair<Rule>) -> Result<MatchHint, ParseError> {
    let rule = pair.as_rule();
    let identifiers = pair
        .into_inner()
        .filter(|item| item.as_rule() == Rule::ident)
        .map(ident_name)
        .collect::<Vec<_>>();
    match rule {
        Rule::using_index_hint => match identifiers.as_slice() {
            [variable, label, property] => Ok(MatchHint::Index {
                variable: variable.clone(),
                label: label.clone(),
                property: property.clone(),
            }),
            _ => Err(ParseError::Unexpected(
                "USING INDEX: expected variable, label, and property".into(),
            )),
        },
        Rule::using_scan_hint => match identifiers.as_slice() {
            [variable, label] => Ok(MatchHint::Scan {
                variable: variable.clone(),
                label: label.clone(),
            }),
            _ => Err(ParseError::Unexpected(
                "USING SCAN: expected variable and label".into(),
            )),
        },
        Rule::using_join_hint if !identifiers.is_empty() => Ok(MatchHint::Join {
            variables: identifiers,
        }),
        Rule::using_join_hint => Err(ParseError::Unexpected(
            "USING JOIN ON: expected at least one variable".into(),
        )),
        r => Err(unexpected("MATCH hint", r)),
    }
}

fn walk_order_by(pair: Pair<Rule>) -> Result<Vec<OrderItem>, ParseError> {
    let mut items = Vec::new();
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::order_items {
            for oi in inner.into_inner() {
                items.push(walk_order_item(oi)?);
            }
        }
    }
    Ok(items)
}

fn walk_order_item(pair: Pair<Rule>) -> Result<OrderItem, ParseError> {
    let mut iter = pair.into_inner();
    let expr_pair = iter
        .next()
        .ok_or_else(|| ParseError::Unexpected("order_item: empty".into()))?;
    let expr = walk_expr(expr_pair)?;
    let mut desc = false;
    for d in iter {
        if d.as_rule() == Rule::order_dir {
            desc = d.as_str().eq_ignore_ascii_case("DESC");
        }
    }
    Ok(OrderItem { expr, desc })
}

fn walk_return(pair: Pair<Rule>) -> Result<ReturnClause, ParseError> {
    let mut items = Vec::new();
    let mut distinct = false;
    let mut include_existing = false;
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::return_items {
            for ri in inner.into_inner() {
                match ri.as_rule() {
                    Rule::kw_distinct => distinct = true,
                    Rule::return_all => {
                        include_existing = true;
                        items.extend(
                            ri.into_inner()
                                .map(walk_return_item)
                                .collect::<Result<Vec<_>, _>>()?,
                        );
                    }
                    Rule::return_item_list => {
                        items.extend(
                            ri.into_inner()
                                .map(walk_return_item)
                                .collect::<Result<Vec<_>, _>>()?,
                        );
                    }
                    r => return Err(unexpected("return items", r)),
                }
            }
        }
    }
    Ok(ReturnClause {
        distinct,
        include_existing,
        items,
    })
}

fn walk_return_item(pair: Pair<Rule>) -> Result<ReturnItem, ParseError> {
    let mut iter = pair.into_inner();
    let expr_pair = iter
        .next()
        .ok_or_else(|| ParseError::Unexpected("return_item: empty".into()))?;
    let expr = walk_expr(expr_pair)?;
    let mut alias = None;
    for a in iter {
        if a.as_rule() == Rule::alias {
            for ai in a.into_inner() {
                if ai.as_rule() == Rule::ident {
                    alias = Some(ident_name(ai));
                }
            }
        }
    }
    Ok(ReturnItem { expr, alias })
}

// --- patterns -------------------------------------------------------------

fn walk_pattern(pair: Pair<Rule>) -> Result<Pattern, ParseError> {
    let inner = first_inner(pair, "pattern")?;
    match inner.as_rule() {
        Rule::named_pattern => walk_named_pattern(inner),
        Rule::pattern_path => walk_pattern_path(inner, None),
        Rule::shortest_path_pattern => walk_shortest_path_pattern(inner),
        r => Err(unexpected("pattern", r)),
    }
}

fn walk_named_pattern(pair: Pair<Rule>) -> Result<Pattern, ParseError> {
    let mut inner = pair.into_inner();
    let variable = ident_name(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("named path: missing variable".into()))?,
    );
    let body = inner
        .next()
        .ok_or_else(|| ParseError::Unexpected("named path: missing pattern".into()))?;
    let mut pattern = match body.as_rule() {
        Rule::pattern_path => walk_pattern_path(body, None)?,
        Rule::shortest_path_pattern => walk_shortest_path_pattern(body)?,
        rule => return Err(unexpected("named path", rule)),
    };
    pattern.path_variable = Some(variable);
    Ok(pattern)
}

fn walk_shortest_path_pattern(pair: Pair<Rule>) -> Result<Pattern, ParseError> {
    let mut shortest = None;
    let mut path = None;
    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::shortest_path_kind => {
                shortest = Some(if item.as_str().eq_ignore_ascii_case("shortestPath") {
                    ShortestPathMode::Single
                } else {
                    ShortestPathMode::All
                });
            }
            Rule::pattern_path => path = Some(item),
            r => return Err(unexpected("shortest path", r)),
        }
    }
    walk_pattern_path(
        path.ok_or_else(|| ParseError::Unexpected("shortest path: missing path".into()))?,
        shortest,
    )
}

fn walk_pattern_path(
    pair: Pair<Rule>,
    shortest: Option<ShortestPathMode>,
) -> Result<Pattern, ParseError> {
    let mut iter = pair.into_inner();
    let anchor = walk_node(
        iter.next()
            .ok_or_else(|| ParseError::Unexpected("pattern: missing anchor".into()))?,
    )?;
    let mut chain = Vec::new();
    for c in iter {
        if c.as_rule() == Rule::rel_chain {
            let mut ci = c.into_inner();
            let rel = walk_rel(
                ci.next()
                    .ok_or_else(|| ParseError::Unexpected("rel_chain: missing rel".into()))?,
            )?;
            let node = walk_node(
                ci.next()
                    .ok_or_else(|| ParseError::Unexpected("rel_chain: missing node".into()))?,
            )?;
            chain.push(RelChain { rel, node });
        }
    }
    Ok(Pattern {
        path_variable: None,
        anchor,
        chain,
        shortest,
    })
}

fn walk_node(pair: Pair<Rule>) -> Result<NodePattern, ParseError> {
    let mut var = None;
    let mut labels = Vec::new();
    let mut properties = Vec::new();
    let mut property_map = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => var = Some(ident_name(inner)),
            Rule::labels => {
                for l in inner.into_inner() {
                    if l.as_rule() == Rule::label {
                        for li in l.into_inner() {
                            if li.as_rule() == Rule::ident {
                                labels.push(ident_name(li));
                            }
                        }
                    }
                }
            }
            Rule::pattern_properties => {
                let value = first_inner(inner, "node properties")?;
                match value.as_rule() {
                    Rule::map_literal => properties = walk_map_entries(value)?,
                    Rule::param => property_map = Some(walk_expr(value)?),
                    rule => return Err(unexpected("node properties", rule)),
                }
            }
            _ => {}
        }
    }
    Ok(NodePattern {
        var,
        labels,
        properties,
        property_map,
    })
}

fn walk_rel(pair: Pair<Rule>) -> Result<RelPattern, ParseError> {
    let inner = first_inner(pair, "rel_pattern")?;
    let direction = match inner.as_rule() {
        Rule::rel_left => Direction::Incoming,
        Rule::rel_right => Direction::Outgoing,
        Rule::rel_undirected => Direction::Undirected,
        r => return Err(unexpected("rel direction", r)),
    };

    let mut var = None;
    let mut types = Vec::new();
    let mut properties = Vec::new();
    let mut property_map = None;
    let mut range = None;
    for d in inner.into_inner() {
        if d.as_rule() == Rule::rel_detail {
            for di in d.into_inner() {
                match di.as_rule() {
                    Rule::ident => var = Some(ident_name(di)),
                    Rule::rel_types => {
                        for t in di.into_inner() {
                            if t.as_rule() == Rule::ident {
                                types.push(ident_name(t));
                            }
                        }
                    }
                    Rule::pattern_properties => {
                        let value = first_inner(di, "relationship properties")?;
                        match value.as_rule() {
                            Rule::map_literal => properties = walk_map_entries(value)?,
                            Rule::param => property_map = Some(walk_expr(value)?),
                            rule => return Err(unexpected("relationship properties", rule)),
                        }
                    }
                    Rule::rel_range => range = Some(walk_relationship_range(di)?),
                    _ => {}
                }
            }
        }
    }
    Ok(RelPattern {
        var,
        direction,
        types,
        properties,
        property_map,
        range,
    })
}

fn walk_relationship_range(pair: Pair<Rule>) -> Result<RelationshipRange, ParseError> {
    let Some(bounds) = pair.into_inner().next() else {
        return Ok(RelationshipRange {
            start: None,
            end: None,
        });
    };

    match bounds.as_rule() {
        Rule::fixed_length => {
            let length = parse_range_bound(first_inner(bounds, "fixed relationship length")?)?;
            Ok(RelationshipRange {
                start: Some(length),
                end: Some(length),
            })
        }
        Rule::range_bounds => {
            let mut start = None;
            let mut end = None;
            for bound in bounds.into_inner() {
                match bound.as_rule() {
                    Rule::range_start => {
                        start = Some(parse_range_bound(first_inner(bound, "range start")?)?)
                    }
                    Rule::range_end => {
                        end = Some(parse_range_bound(first_inner(bound, "range end")?)?)
                    }
                    r => return Err(unexpected("relationship range", r)),
                }
            }
            Ok(RelationshipRange { start, end })
        }
        r => Err(unexpected("relationship range", r)),
    }
}

fn parse_range_bound(pair: Pair<Rule>) -> Result<u64, ParseError> {
    parse_unsigned_integer(pair.as_str())
}

/// Walk a `map_literal` rule's children and return the (key, value) pairs.
fn walk_map_entries(pair: Pair<Rule>) -> Result<Vec<(String, Expr)>, ParseError> {
    let mut out = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() != Rule::map_entry {
            continue;
        }
        let mut iter = entry.into_inner();
        let key_pair = iter
            .next()
            .ok_or_else(|| ParseError::Unexpected("map_entry: missing key".into()))?;
        let val_pair = iter
            .next()
            .ok_or_else(|| ParseError::Unexpected("map_entry: missing value".into()))?;
        let key = ident_name(key_pair);
        let val = walk_expr(val_pair)?;
        out.push((key, val));
    }
    Ok(out)
}

// --- expressions ----------------------------------------------------------

fn walk_expr(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    match pair.as_rule() {
        Rule::expr => walk_expr(first_inner(pair, "expr")?),
        Rule::or_expr => walk_left_assoc_no_op(pair, BinOp::Or),
        Rule::xor_expr => walk_left_assoc_no_op(pair, BinOp::Xor),
        Rule::and_expr => walk_left_assoc_no_op(pair, BinOp::And),
        Rule::not_op => {
            let inner = first_operand(pair, "not")?;
            Ok(Expr::Unary {
                op: UnOp::Not,
                operand: Box::new(walk_expr(inner)?),
            })
        }
        Rule::cmp_expr => walk_cmp(pair),
        Rule::add_expr => walk_left_assoc_with_op(pair, |s| match s {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            _ => None,
        }),
        Rule::mul_expr => walk_left_assoc_with_op(pair, |s| match s {
            "*" => Some(BinOp::Mul),
            "/" => Some(BinOp::Div),
            "%" => Some(BinOp::Mod),
            _ => None,
        }),
        Rule::pow_expr => walk_right_assoc_pow(pair),
        Rule::pos_op => {
            let inner = first_inner(pair, "positive")?;
            Ok(Expr::Unary {
                op: UnOp::Pos,
                operand: Box::new(walk_expr(inner)?),
            })
        }
        Rule::neg_op => {
            let inner = first_inner(pair, "neg")?;
            Ok(Expr::Unary {
                op: UnOp::Neg,
                operand: Box::new(walk_expr(inner)?),
            })
        }
        Rule::regex_expr => walk_left_assoc_with_op(pair, |s| match s {
            "=~" => Some(BinOp::RegexMatch),
            _ => None,
        }),
        Rule::postfix_expr => walk_postfix(pair),
        Rule::pattern_expression => walk_pattern_expression(pair),
        Rule::paren_expr => walk_expr(first_inner(pair, "paren")?),
        Rule::case_expression => walk_case_expression(pair),
        Rule::pattern_comprehension => walk_pattern_comprehension(pair),
        Rule::list_comprehension => walk_list_comprehension(pair),
        Rule::filter_expression => walk_filter_expression(pair),
        Rule::extract_expression => walk_extract_expression(pair),
        Rule::reduce_expression => walk_reduce_expression(pair),
        Rule::collection_predicate => walk_collection_predicate(pair),
        Rule::function_call => walk_function_call(pair),
        Rule::var_ref => {
            let inner = first_inner(pair, "var_ref")?;
            Ok(Expr::Variable(ident_name(inner)))
        }
        Rule::param => Ok(Expr::Param(ident_name(first_inner(pair, "parameter")?))),
        Rule::integer => {
            let s = pair.as_str();
            let value = parse_unsigned_integer(s).and_then(|value| {
                i64::try_from(value).map_err(|_| ParseError::InvalidInt(s.into()))
            })?;
            Ok(Expr::Literal(Literal::Int(value)))
        }
        Rule::float => {
            let s = pair.as_str();
            let value = s
                .parse::<f64>()
                .map_err(|_| ParseError::InvalidFloat(s.into()))?;
            if !value.is_finite() {
                return Err(ParseError::InvalidFloat(s.into()));
            }
            Ok(Expr::Literal(Literal::Float(value)))
        }
        Rule::string_lit => Ok(Expr::Literal(Literal::String(decode_string_literal(
            pair.as_str(),
        )?))),
        Rule::bool_lit => Ok(Expr::Literal(Literal::Bool(
            pair.as_str().eq_ignore_ascii_case("true"),
        ))),
        Rule::null_lit => Ok(Expr::Literal(Literal::Null)),
        Rule::list_lit => {
            let mut items = Vec::new();
            for item in pair.into_inner() {
                items.push(walk_expr(item)?);
            }
            Ok(Expr::List(items))
        }
        Rule::map_literal => {
            let entries = walk_map_entries(pair)?;
            Ok(Expr::Map(entries))
        }
        r => Err(unexpected("walk_expr", r)),
    }
}

fn parse_unsigned_integer(raw: &str) -> Result<u64, ParseError> {
    let (digits, radix) = if let Some(digits) = raw.strip_prefix("0x") {
        (digits, 16)
    } else if raw.len() > 1 && raw.starts_with('0') {
        (&raw[1..], 8)
    } else {
        (raw, 10)
    };
    u64::from_str_radix(digits, radix).map_err(|_| ParseError::InvalidInt(raw.into()))
}

fn walk_pattern_expression(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let pattern = match first_inner(pair, "pattern expression")? {
        path if path.as_rule() == Rule::pattern_expression_path => walk_pattern_path(path, None)?,
        shortest if shortest.as_rule() == Rule::shortest_path_pattern => {
            walk_shortest_path_pattern(shortest)?
        }
        other => return Err(unexpected("pattern expression", other.as_rule())),
    };
    Ok(Expr::PatternExpression {
        pattern: Box::new(pattern),
    })
}

fn walk_filter_expression(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut inner = pair.into_inner().filter(|item| !is_kw(item));
    let variable = ident_name(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("FILTER: missing variable".into()))?,
    );
    let source = walk_expr(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("FILTER: missing source".into()))?,
    )?;
    let predicate = inner.next().map(walk_expr).transpose()?.map(Box::new);
    Ok(Expr::Filter {
        variable,
        source: Box::new(source),
        predicate,
    })
}

fn walk_extract_expression(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut inner = pair.into_inner().filter(|item| !is_kw(item));
    let variable = ident_name(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("EXTRACT: missing variable".into()))?,
    );
    let source = walk_expr(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("EXTRACT: missing source".into()))?,
    )?;
    let projection = inner.next().map(walk_expr).transpose()?.map(Box::new);
    Ok(Expr::Extract {
        variable,
        source: Box::new(source),
        projection,
    })
}

fn walk_reduce_expression(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut inner = pair.into_inner().filter(|item| !is_kw(item));
    let accumulator = ident_name(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("REDUCE: missing accumulator".into()))?,
    );
    let initial = walk_expr(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("REDUCE: missing initial value".into()))?,
    )?;
    let variable = ident_name(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("REDUCE: missing variable".into()))?,
    );
    let source = walk_expr(
        inner
            .next()
            .ok_or_else(|| ParseError::Unexpected("REDUCE: missing source".into()))?,
    )?;
    let expression = inner.next().map(walk_expr).transpose()?.map(Box::new);
    Ok(Expr::Reduce {
        accumulator,
        initial: Box::new(initial),
        variable,
        source: Box::new(source),
        expression,
    })
}

fn walk_pattern_comprehension(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut path_variable = None;
    let mut pattern = None;
    let mut expressions = Vec::new();
    let mut has_where = false;
    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::ident => path_variable = Some(ident_name(item)),
            Rule::pattern_expression_path => {
                pattern = Some(walk_pattern_path(item, None)?);
            }
            Rule::kw_where => has_where = true,
            Rule::expr => expressions.push(walk_expr(item)?),
            r => return Err(unexpected("pattern comprehension", r)),
        }
    }
    let mut expressions = expressions.into_iter();
    let predicate = if has_where {
        Some(Box::new(expressions.next().ok_or_else(|| {
            ParseError::Unexpected("pattern comprehension: missing WHERE predicate".into())
        })?))
    } else {
        None
    };
    let projection = expressions.next().ok_or_else(|| {
        ParseError::Unexpected("pattern comprehension: missing projection".into())
    })?;

    Ok(Expr::PatternComprehension {
        path_variable,
        pattern: Box::new(pattern.ok_or_else(|| {
            ParseError::Unexpected("pattern comprehension: missing pattern".into())
        })?),
        predicate,
        projection: Box::new(projection),
    })
}

fn walk_list_comprehension(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut inner = pair.into_inner();
    let variable =
        ident_name(inner.next().ok_or_else(|| {
            ParseError::Unexpected("list comprehension: missing variable".into())
        })?);
    let mut expressions = Vec::new();
    let mut has_where = false;
    for item in inner {
        match item.as_rule() {
            Rule::kw_in => {}
            Rule::kw_where => has_where = true,
            Rule::expr => expressions.push(walk_expr(item)?),
            r => return Err(unexpected("list comprehension", r)),
        }
    }
    let mut expressions = expressions.into_iter();
    let source = expressions
        .next()
        .ok_or_else(|| ParseError::Unexpected("list comprehension: missing source".into()))?;
    let predicate = if has_where {
        Some(Box::new(expressions.next().ok_or_else(|| {
            ParseError::Unexpected("list comprehension: missing WHERE predicate".into())
        })?))
    } else {
        None
    };
    let projection = expressions.next().map(Box::new);

    Ok(Expr::ListComprehension {
        variable,
        source: Box::new(source),
        predicate,
        projection,
    })
}

fn walk_collection_predicate(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut inner = pair.into_inner();
    let kind_pair = inner.next().ok_or_else(|| {
        ParseError::Unexpected("collection predicate: missing predicate kind".into())
    })?;
    let kind = match kind_pair.as_str().to_ascii_lowercase().as_str() {
        "all" => CollectionPredicateKind::All,
        "any" => CollectionPredicateKind::Any,
        "none" => CollectionPredicateKind::None,
        "single" => CollectionPredicateKind::Single,
        _ => return Err(unexpected("collection predicate kind", kind_pair.as_rule())),
    };
    let variable =
        ident_name(inner.next().ok_or_else(|| {
            ParseError::Unexpected("collection predicate: missing variable".into())
        })?);
    let mut expressions = inner.filter(|item| !is_kw(item)).map(walk_expr);
    let source = expressions
        .next()
        .ok_or_else(|| ParseError::Unexpected("collection predicate: missing source".into()))??;
    let predicate = expressions.next().transpose()?.map(Box::new);

    Ok(Expr::CollectionPredicate {
        kind,
        variable,
        source: Box::new(source),
        predicate,
    })
}

fn walk_case_expression(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut operand = None;
    let mut alternatives = Vec::new();
    let mut else_expr = None;

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::expr if alternatives.is_empty() && operand.is_none() => {
                operand = Some(Box::new(walk_expr(item)?));
            }
            Rule::case_alternative => alternatives.push(walk_case_alternative(item)?),
            Rule::case_default => {
                else_expr = Some(Box::new(walk_expr(first_operand(item, "case ELSE")?)?));
            }
            r if is_kw_rule(r) => {}
            r => return Err(unexpected("case expression", r)),
        }
    }

    Ok(Expr::Case {
        operand,
        alternatives,
        else_expr,
    })
}

fn walk_case_alternative(pair: Pair<Rule>) -> Result<CaseAlternative, ParseError> {
    let mut expressions = pair.into_inner().filter(|item| !is_kw(item)).map(walk_expr);
    let when = expressions
        .next()
        .ok_or_else(|| ParseError::Unexpected("case WHEN: missing predicate".into()))??;
    let then = expressions
        .next()
        .ok_or_else(|| ParseError::Unexpected("case THEN: missing value".into()))??;
    Ok(CaseAlternative { when, then })
}

fn walk_function_call(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut inner = pair.into_inner();
    let name_pair = inner
        .next()
        .ok_or_else(|| ParseError::Unexpected("function call: missing name".into()))?;
    let name = name_pair
        .into_inner()
        .map(ident_name)
        .collect::<Vec<_>>()
        .join(".");

    let mut distinct = false;
    let arguments = match inner.next() {
        None => FunctionArguments::Expressions(Vec::new()),
        Some(body) => {
            let mut expressions = Vec::new();
            let mut wildcard = false;
            for item in body.into_inner() {
                match item.as_rule() {
                    Rule::kw_distinct => distinct = true,
                    Rule::wildcard_argument => wildcard = true,
                    Rule::expression_list => {
                        expressions = item
                            .into_inner()
                            .map(walk_expr)
                            .collect::<Result<Vec<_>, _>>()?;
                    }
                    r => return Err(unexpected("function call body", r)),
                }
            }
            if wildcard {
                FunctionArguments::Wildcard
            } else {
                FunctionArguments::Expressions(expressions)
            }
        }
    };

    Ok(Expr::FunctionCall {
        name,
        distinct,
        arguments,
    })
}

/// `or_expr` and `and_expr` interleave operand - kw_or/kw_and - operand - ...
/// We walk operands and skip the keyword tokens.
fn walk_left_assoc_no_op(pair: Pair<Rule>, op: BinOp) -> Result<Expr, ParseError> {
    let mut acc: Option<Expr> = None;
    for inner in pair.into_inner() {
        if is_kw(&inner) {
            continue;
        }
        let next = walk_expr(inner)?;
        acc = Some(match acc {
            None => next,
            Some(lhs) => Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(next),
            },
        });
    }
    acc.ok_or_else(|| ParseError::Unexpected("left_assoc: no operands".into()))
}

/// `add_expr` and `mul_expr` interleave operand, op, operand, op, ...
fn walk_left_assoc_with_op<F>(pair: Pair<Rule>, op_for: F) -> Result<Expr, ParseError>
where
    F: Fn(&str) -> Option<BinOp>,
{
    let mut iter = pair.into_inner();
    let first = iter
        .next()
        .ok_or_else(|| ParseError::Unexpected("arith: empty".into()))?;
    let mut acc = walk_expr(first)?;
    while let Some(op_pair) = iter.next() {
        let op = op_for(op_pair.as_str())
            .ok_or_else(|| ParseError::Unexpected(format!("arith op: {}", op_pair.as_str())))?;
        let rhs_pair = iter
            .next()
            .ok_or_else(|| ParseError::Unexpected("arith: missing rhs".into()))?;
        let rhs = walk_expr(rhs_pair)?;
        acc = Expr::Binary {
            op,
            lhs: Box::new(acc),
            rhs: Box::new(rhs),
        };
    }
    Ok(acc)
}

fn walk_right_assoc_pow(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut iter = pair.into_inner();
    let lhs = walk_expr(
        iter.next()
            .ok_or_else(|| ParseError::Unexpected("power: empty".into()))?,
    )?;
    let Some(op) = iter.next() else {
        return Ok(lhs);
    };
    if op.as_rule() != Rule::pow_op {
        return Err(unexpected("power", op.as_rule()));
    }
    let rhs = walk_expr(
        iter.next()
            .ok_or_else(|| ParseError::Unexpected("power: missing rhs".into()))?,
    )?;
    Ok(Expr::Binary {
        op: BinOp::Pow,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    })
}

fn walk_cmp(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut iter = pair.into_inner();
    let lhs_pair = iter
        .next()
        .ok_or_else(|| ParseError::Unexpected("cmp: empty".into()))?;
    let lhs = walk_expr(lhs_pair)?;
    let tail = match iter.next() {
        Some(p) => p,
        None => return Ok(lhs),
    };
    if tail.as_rule() == Rule::rel_comparison_tail {
        let mut operators = Vec::new();
        let mut arguments = vec![lhs];
        for comparison in std::iter::once(tail).chain(iter) {
            let mut parts = comparison.into_inner();
            let op = comparison_op(
                parts
                    .next()
                    .ok_or_else(|| ParseError::Unexpected("comparison: missing op".into()))?
                    .as_str(),
            )?;
            let argument = walk_expr(
                parts
                    .next()
                    .ok_or_else(|| ParseError::Unexpected("comparison: missing rhs".into()))?,
            )?;
            operators.push(op);
            arguments.push(argument);
        }
        if operators.len() == 1 {
            return Ok(Expr::Binary {
                op: operators[0],
                lhs: Box::new(arguments.remove(0)),
                rhs: Box::new(arguments.remove(0)),
            });
        }
        return Ok(Expr::ComparisonChain {
            operators,
            arguments,
        });
    }
    match tail.as_rule() {
        Rule::cmp_op_tail => {
            let mut tail_iter = tail.into_inner();
            let op_pair = tail_iter
                .next()
                .ok_or_else(|| ParseError::Unexpected("cmp_op_tail: missing op".into()))?;
            let rhs_pair = tail_iter
                .next()
                .ok_or_else(|| ParseError::Unexpected("cmp_op_tail: missing rhs".into()))?;
            let op = comparison_op(op_pair.as_str())?;
            let rhs = walk_expr(rhs_pair)?;
            Ok(Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        Rule::in_op_tail => {
            let rhs_pair = first_operand(tail, "in_op_tail")?;
            let rhs = walk_expr(rhs_pair)?;
            Ok(Expr::Binary {
                op: BinOp::In,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        Rule::starts_with_tail => {
            let rhs_pair = first_operand(tail, "starts_with_tail")?;
            let rhs = walk_expr(rhs_pair)?;
            Ok(Expr::Binary {
                op: BinOp::StartsWith,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        Rule::ends_with_tail => {
            let rhs_pair = first_operand(tail, "ends_with_tail")?;
            let rhs = walk_expr(rhs_pair)?;
            Ok(Expr::Binary {
                op: BinOp::EndsWith,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        Rule::contains_tail => {
            let rhs_pair = first_operand(tail, "contains_tail")?;
            let rhs = walk_expr(rhs_pair)?;
            Ok(Expr::Binary {
                op: BinOp::Contains,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        Rule::is_null_tail => Ok(Expr::Unary {
            op: UnOp::IsNull,
            operand: Box::new(lhs),
        }),
        Rule::is_not_null_tail => Ok(Expr::Unary {
            op: UnOp::IsNotNull,
            operand: Box::new(lhs),
        }),
        r => Err(unexpected("cmp tail", r)),
    }
}

fn comparison_op(operator: &str) -> Result<BinOp, ParseError> {
    match operator {
        "=" => Ok(BinOp::Eq),
        "<>" => Ok(BinOp::Neq),
        "<" => Ok(BinOp::Lt),
        "<=" => Ok(BinOp::Lte),
        ">" => Ok(BinOp::Gt),
        ">=" => Ok(BinOp::Gte),
        other => Err(ParseError::Unexpected(format!("cmp op: {other}"))),
    }
}

fn walk_postfix(pair: Pair<Rule>) -> Result<Expr, ParseError> {
    let mut iter = pair.into_inner();
    let first = iter
        .next()
        .ok_or_else(|| ParseError::Unexpected("postfix: empty".into()))?;
    let mut acc = walk_expr(first)?;
    for p in iter {
        match p.as_rule() {
            Rule::prop_access => {
                let key = first_inner(p, "prop_access")?;
                acc = Expr::Property {
                    base: Box::new(acc),
                    key: ident_name(key),
                };
            }
            Rule::label_predicate => {
                let labels = p
                    .into_inner()
                    .filter(|label| label.as_rule() == Rule::label)
                    .map(|label| first_inner(label, "label predicate").map(ident_name))
                    .collect::<Result<Vec<_>, _>>()?;
                acc = Expr::LabelPredicate {
                    expression: Box::new(acc),
                    labels,
                };
            }
            Rule::collection_subscript => {
                acc = Expr::Subscript {
                    base: Box::new(acc),
                    index: Box::new(walk_expr(first_inner(p, "subscript")?)?),
                };
            }
            Rule::collection_slice => {
                let mut start = None;
                let mut end = None;
                for bound in p.into_inner() {
                    match bound.as_rule() {
                        Rule::slice_start => {
                            start = Some(Box::new(walk_expr(first_inner(bound, "slice start")?)?));
                        }
                        Rule::slice_end => {
                            end = Some(Box::new(walk_expr(first_inner(bound, "slice end")?)?));
                        }
                        r => return Err(unexpected("slice", r)),
                    }
                }
                acc = Expr::Slice {
                    base: Box::new(acc),
                    start,
                    end,
                };
            }
            Rule::map_projection => {
                let items = p
                    .into_inner()
                    .map(walk_map_projection_item)
                    .collect::<Result<Vec<_>, _>>()?;
                acc = Expr::MapProjection {
                    base: Box::new(acc),
                    items,
                };
            }
            r => return Err(unexpected("postfix", r)),
        }
    }
    Ok(acc)
}

fn walk_map_projection_item(pair: Pair<Rule>) -> Result<MapProjectionItem, ParseError> {
    match pair.as_rule() {
        Rule::map_projection_literal => {
            let mut inner = pair.into_inner();
            let key = ident_name(
                inner
                    .next()
                    .ok_or_else(|| ParseError::Unexpected("map projection: missing key".into()))?,
            );
            let value =
                walk_expr(inner.next().ok_or_else(|| {
                    ParseError::Unexpected("map projection: missing value".into())
                })?)?;
            Ok(MapProjectionItem::Literal { key, value })
        }
        Rule::map_projection_property => Ok(MapProjectionItem::Property(ident_name(first_inner(
            pair,
            "map projection property",
        )?))),
        Rule::map_projection_variable => Ok(MapProjectionItem::Variable(ident_name(first_inner(
            pair,
            "map projection variable",
        )?))),
        Rule::map_projection_all => Ok(MapProjectionItem::AllProperties),
        r => Err(unexpected("map projection item", r)),
    }
}

fn ident_name(pair: Pair<Rule>) -> String {
    let raw = pair.as_str();
    if raw.starts_with('`') && raw.ends_with('`') {
        raw[1..raw.len() - 1].replace("``", "`")
    } else {
        raw.to_string()
    }
}

fn decode_string_literal(raw: &str) -> Result<String, ParseError> {
    let mut chars = raw[1..raw.len() - 1].chars().peekable();
    let mut decoded = String::new();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }

        let escape = chars
            .next()
            .ok_or_else(|| ParseError::InvalidString("trailing backslash".into()))?;
        match escape {
            'a' => decoded.push('\u{0007}'),
            'b' => decoded.push('\u{0008}'),
            'f' => decoded.push('\u{000c}'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'v' => decoded.push('\u{000b}'),
            '\\' => decoded.push('\\'),
            '\'' => decoded.push('\''),
            '"' => decoded.push('"'),
            '?' => decoded.push('?'),
            'u' => {
                let code = read_hex_escape(&mut chars, 4)?;
                if (0xd800..=0xdbff).contains(&code) {
                    if chars.next() != Some('\\') || chars.next() != Some('u') {
                        return Err(ParseError::InvalidString(
                            "high surrogate must be followed by a low surrogate".into(),
                        ));
                    }
                    let low = read_hex_escape(&mut chars, 4)?;
                    if !(0xdc00..=0xdfff).contains(&low) {
                        return Err(ParseError::InvalidString(
                            "high surrogate must be followed by a low surrogate".into(),
                        ));
                    }
                    let scalar = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                    decoded.push(char::from_u32(scalar).ok_or_else(|| {
                        ParseError::InvalidString(format!("invalid Unicode scalar U+{scalar:04X}"))
                    })?);
                } else {
                    decoded.push(decode_scalar(code)?);
                }
            }
            'U' => decoded.push(decode_scalar(read_hex_escape(&mut chars, 8)?)?),
            other => {
                return Err(ParseError::InvalidString(format!(
                    "unknown escape sequence \\{other}"
                )));
            }
        }
    }
    Ok(decoded)
}

fn read_hex_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    digits: usize,
) -> Result<u32, ParseError> {
    let mut value = 0_u32;
    for _ in 0..digits {
        let digit = chars
            .next()
            .and_then(|ch| ch.to_digit(16))
            .ok_or_else(|| ParseError::InvalidString("incomplete Unicode escape".into()))?;
        value = (value << 4) | digit;
    }
    Ok(value)
}

fn decode_scalar(value: u32) -> Result<char, ParseError> {
    char::from_u32(value)
        .ok_or_else(|| ParseError::InvalidString(format!("invalid Unicode scalar U+{value:04X}")))
}

// --- helpers --------------------------------------------------------------

fn first_inner<'a>(pair: Pair<'a, Rule>, what: &'static str) -> Result<Pair<'a, Rule>, ParseError> {
    pair.into_inner()
        .next()
        .ok_or_else(|| ParseError::Unexpected(format!("{what}: missing inner")))
}

/// Return the first child that isn't a `kw_*` token.
fn first_operand<'a>(
    pair: Pair<'a, Rule>,
    what: &'static str,
) -> Result<Pair<'a, Rule>, ParseError> {
    for inner in pair.into_inner() {
        if !is_kw(&inner) {
            return Ok(inner);
        }
    }
    Err(ParseError::Unexpected(format!("{what}: missing operand")))
}

fn is_kw(p: &Pair<Rule>) -> bool {
    is_kw_rule(p.as_rule())
}

fn is_kw_rule(rule: Rule) -> bool {
    matches!(
        rule,
        Rule::kw_match
            | Rule::kw_optional
            | Rule::kw_where
            | Rule::kw_return
            | Rule::kw_as
            | Rule::kw_order
            | Rule::kw_by
            | Rule::kw_asc
            | Rule::kw_desc
            | Rule::kw_limit
            | Rule::kw_skip
            | Rule::kw_with
            | Rule::kw_and
            | Rule::kw_or
            | Rule::kw_xor
            | Rule::kw_not
            | Rule::kw_in
            | Rule::kw_true
            | Rule::kw_false
            | Rule::kw_null
            | Rule::kw_is
            | Rule::kw_starts
            | Rule::kw_ends
            | Rule::kw_contains
            | Rule::kw_distinct
            | Rule::kw_create
            | Rule::kw_merge
            | Rule::kw_set
            | Rule::kw_delete
            | Rule::kw_detach
            | Rule::kw_unwind
            | Rule::kw_on
            | Rule::kw_call
            | Rule::kw_yield
            | Rule::kw_case
            | Rule::kw_when
            | Rule::kw_then
            | Rule::kw_else
            | Rule::kw_end
            | Rule::kw_union
            | Rule::kw_all
            | Rule::kw_any
            | Rule::kw_none
            | Rule::kw_single
            | Rule::kw_filter
            | Rule::kw_extract
            | Rule::kw_reduce
            | Rule::kw_shortest_path
            | Rule::kw_all_shortest_paths
    )
}

fn unexpected(ctx: &str, rule: Rule) -> ParseError {
    ParseError::Unexpected(format!("{ctx}: {rule:?}"))
}
