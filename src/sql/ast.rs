use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataType {
    Integer,
    Text,
    Boolean,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Null,
    Integer(i64),
    Text(String),
    Boolean(bool),
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(formatter, "NULL"),
            Self::Integer(value) => write!(formatter, "{value}"),
            Self::Text(value) => write!(formatter, "{value}"),
            Self::Boolean(value) => write!(formatter, "{value}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub primary_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Column(String),
    Literal(Value),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Projection {
    All,
    Columns(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Select {
    pub table: String,
    pub projection: Projection,
    pub filter: Option<Expr>,
    pub order_by: Option<(String, bool)>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    CreateTable {
        table: String,
        columns: Vec<ColumnDef>,
    },
    Insert {
        table: String,
        columns: Vec<String>,
        values: Vec<Value>,
    },
    Select(Select),
    Update {
        table: String,
        assignments: Vec<(String, Value)>,
        filter: Option<Expr>,
    },
    Delete {
        table: String,
        filter: Option<Expr>,
    },
    Begin,
    Commit,
    Rollback,
    Explain(Select),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlResult {
    Affected(usize),
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    Begun,
    Committed,
    RolledBack,
    Explain(String),
}
