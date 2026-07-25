//! Abstract syntax tree for the openCypher subset supported in v0.

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Clause {
    Match(MatchClause),
    Create(CreateClause),
    Merge(MergeClause),
    Set(SetClause),
    Remove(RemoveClause),
    Delete(DeleteClause),
    Unwind(UnwindClause),
    Where(Expr),
    Return(ReturnClause),
    /// Pipeline break: project the current row set and pass it to the
    /// next clause. Semantics mirror RETURN but the query continues.
    With(ReturnClause),
    OrderBy(Vec<OrderItem>),
    Limit(Expr),
    Skip(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateClause {
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
pub struct OrderItem {
    pub expr: Expr,
    pub desc: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchClause {
    pub optional: bool,
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub anchor: NodePattern,
    pub chain: Vec<RelChain>,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelPattern {
    pub var: Option<String>,
    pub direction: Direction,
    pub types: Vec<String>,
    /// Same shape as `NodePattern.properties` for relationship
    /// property equalities.
    pub properties: Vec<(String, Expr)>,
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
    Property {
        base: Box<Expr>,
        key: String,
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
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    In,
    StartsWith,
    EndsWith,
    Contains,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
    IsNull,
    IsNotNull,
}
