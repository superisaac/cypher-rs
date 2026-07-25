//! Abstract syntax tree for the openCypher subset supported in v0.

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// Options attached to the first statement.
    pub options: Vec<QueryOption>,
    /// Clauses in the first statement. Kept as the primary field for
    /// compatibility with callers that parse a single statement.
    pub clauses: Vec<Clause>,
    /// Clauses in each statement after the first.
    pub additional_statements: Vec<Vec<Clause>>,
    /// Options for each statement in `additional_statements`.
    pub additional_statement_options: Vec<Vec<QueryOption>>,
}

impl Query {
    pub fn new(clauses: Vec<Clause>) -> Self {
        Self {
            options: Vec::new(),
            clauses,
            additional_statements: Vec::new(),
            additional_statement_options: Vec::new(),
        }
    }

    pub fn statement_count(&self) -> usize {
        1 + self.additional_statements.len()
    }

    pub fn statements(&self) -> impl Iterator<Item = &[Clause]> {
        std::iter::once(self.clauses.as_slice())
            .chain(self.additional_statements.iter().map(Vec::as_slice))
    }

    pub fn clause_count(&self) -> usize {
        self.statements().map(<[_]>::len).sum()
    }

    pub fn option_count(&self) -> usize {
        self.statement_options().map(<[_]>::len).sum()
    }

    pub fn statement_options(&self) -> impl Iterator<Item = &[QueryOption]> {
        std::iter::once(self.options.as_slice())
            .chain(self.additional_statement_options.iter().map(Vec::as_slice))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryOption {
    Explain,
    Profile,
    Cypher {
        version: Option<String>,
        settings: Vec<(String, String)>,
    },
    UsingPeriodicCommit {
        limit: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Clause {
    SchemaCommand(SchemaCommand),
    Match(MatchClause),
    Create(CreateClause),
    Merge(MergeClause),
    Set(SetClause),
    Remove(RemoveClause),
    Delete(DeleteClause),
    Unwind(UnwindClause),
    Foreach(ForeachClause),
    Start(StartClause),
    LoadCsv(LoadCsvClause),
    Call(CallClause),
    Where(Expr),
    Return(ReturnClause),
    /// Pipeline break: project the current row set and pass it to the
    /// next clause. Semantics mirror RETURN but the query continues.
    With(ReturnClause),
    OrderBy(Vec<OrderItem>),
    Limit(Expr),
    Skip(Expr),
    Union(UnionClause),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaCommand {
    CreateIndex {
        label: String,
        properties: Vec<String>,
    },
    DropIndex {
        label: String,
        properties: Vec<String>,
    },
    CreateNodeConstraint {
        variable: String,
        label: String,
        expression: Expr,
        unique: bool,
    },
    CreateRelationshipConstraint {
        variable: String,
        relationship_type: String,
        expression: Expr,
    },
    DropNodeConstraint {
        variable: String,
        label: String,
        expression: Expr,
        unique: bool,
    },
    DropRelationshipConstraint {
        variable: String,
        relationship_type: String,
        expression: Expr,
    },
}

impl SchemaCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::CreateIndex { .. } => "CREATE INDEX",
            Self::DropIndex { .. } => "DROP INDEX",
            Self::CreateNodeConstraint { .. } | Self::CreateRelationshipConstraint { .. } => {
                "CREATE CONSTRAINT"
            }
            Self::DropNodeConstraint { .. } | Self::DropRelationshipConstraint { .. } => {
                "DROP CONSTRAINT"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnionClause {
    pub all: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateClause {
    pub unique: bool,
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergeClause {
    pub pattern: Pattern,
    pub actions: Vec<MergeAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergeAction {
    pub kind: MergeActionKind,
    pub items: Vec<SetItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeActionKind {
    OnMatch,
    OnCreate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetClause {
    pub items: Vec<SetItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SetItem {
    Property {
        property: Expr,
        value: Expr,
    },
    AllProperties {
        variable: String,
        value: Expr,
    },
    MergeProperties {
        variable: String,
        value: Expr,
    },
    Labels {
        variable: String,
        labels: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoveClause {
    pub items: Vec<RemoveItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoveItem {
    Property(Expr),
    Labels {
        variable: String,
        labels: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeleteClause {
    pub detach: bool,
    pub expressions: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnwindClause {
    pub expr: Expr,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForeachClause {
    pub variable: String,
    pub expression: Expr,
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadCsvClause {
    pub with_headers: bool,
    pub url: Expr,
    pub variable: String,
    pub field_terminator: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StartClause {
    pub points: Vec<StartPoint>,
    pub predicate: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StartPoint {
    pub variable: String,
    pub entity: StartEntity,
    pub lookup: StartLookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartEntity {
    Node,
    Relationship,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StartLookup {
    All,
    Ids(Vec<i64>),
    Index {
        name: String,
        property: Option<String>,
        value: Expr,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallClause {
    pub name: String,
    pub arguments: Vec<Expr>,
    pub yields: Vec<YieldItem>,
    pub predicate: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YieldItem {
    pub field: String,
    pub alias: Option<String>,
}

impl YieldItem {
    pub fn binding(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.field)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    pub expr: Expr,
    pub desc: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchClause {
    pub optional: bool,
    pub patterns: Vec<Pattern>,
    pub hints: Vec<MatchHint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchHint {
    Index {
        variable: String,
        label: String,
        property: String,
    },
    Scan {
        variable: String,
        label: String,
    },
    Join {
        variables: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub path_variable: Option<String>,
    pub anchor: NodePattern,
    pub chain: Vec<RelChain>,
    pub shortest: Option<ShortestPathMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortestPathMode {
    Single,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelChain {
    pub rel: RelPattern,
    pub node: NodePattern,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodePattern {
    pub var: Option<String>,
    pub labels: Vec<String>,
    /// Property equalities encoded by a `{key: value, ...}` literal
    /// inside the node pattern. The lowerer turns each entry into a
    /// `Filter` predicate of the form `var.key = value`.
    pub properties: Vec<(String, Expr)>,
    /// A parameter supplying the complete property map at runtime.
    pub property_map: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelPattern {
    pub var: Option<String>,
    pub direction: Direction,
    pub types: Vec<String>,
    /// Same shape as `NodePattern.properties` for relationship
    /// property equalities.
    pub properties: Vec<(String, Expr)>,
    /// A parameter supplying the complete property map at runtime.
    pub property_map: Option<Expr>,
    pub range: Option<RelationshipRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationshipRange {
    pub start: Option<u64>,
    pub end: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Undirected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnClause {
    pub distinct: bool,
    /// Preserve every variable currently in scope before adding explicit items.
    pub include_existing: bool,
    pub items: Vec<ReturnItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnItem {
    pub expr: Expr,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    List(Vec<Expr>),
    Map(Vec<(String, Expr)>),
    Variable(String),
    Param(String),
    FunctionCall {
        name: String,
        distinct: bool,
        arguments: FunctionArguments,
    },
    ListComprehension {
        variable: String,
        source: Box<Expr>,
        predicate: Option<Box<Expr>>,
        projection: Option<Box<Expr>>,
    },
    CollectionPredicate {
        kind: CollectionPredicateKind,
        variable: String,
        source: Box<Expr>,
        predicate: Option<Box<Expr>>,
    },
    PatternComprehension {
        path_variable: Option<String>,
        pattern: Box<Pattern>,
        predicate: Option<Box<Expr>>,
        projection: Box<Expr>,
    },
    PatternExpression {
        pattern: Box<Pattern>,
    },
    Filter {
        variable: String,
        source: Box<Expr>,
        predicate: Option<Box<Expr>>,
    },
    Extract {
        variable: String,
        source: Box<Expr>,
        projection: Option<Box<Expr>>,
    },
    Reduce {
        accumulator: String,
        initial: Box<Expr>,
        variable: String,
        source: Box<Expr>,
        expression: Option<Box<Expr>>,
    },
    Case {
        operand: Option<Box<Expr>>,
        alternatives: Vec<CaseAlternative>,
        else_expr: Option<Box<Expr>>,
    },
    Property {
        base: Box<Expr>,
        key: String,
    },
    LabelPredicate {
        expression: Box<Expr>,
        labels: Vec<String>,
    },
    Subscript {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Slice {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
    },
    MapProjection {
        base: Box<Expr>,
        items: Vec<MapProjectionItem>,
    },
    ComparisonChain {
        operators: Vec<BinOp>,
        arguments: Vec<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionArguments {
    Expressions(Vec<Expr>),
    Wildcard,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MapProjectionItem {
    Literal { key: String, value: Expr },
    Property(String),
    Variable(String),
    AllProperties,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionPredicateKind {
    All,
    Any,
    None,
    Single,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseAlternative {
    pub when: Expr,
    pub then: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Xor,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    RegexMatch,
    In,
    StartsWith,
    EndsWith,
    Contains,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Pos,
    Neg,
    IsNull,
    IsNotNull,
}
