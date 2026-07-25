//! Rule-based physical planning.

use std::fmt;

use crate::catalog::Schema;

use super::ast::{BinaryOp, Expr, Select, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    FullScan,
    PrimaryKey(Value),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub access: Access,
    pub select: Select,
}

pub fn plan_select(select: &Select, schema: &Schema) -> Plan {
    let primary = &schema.columns[schema.primary_index()].name;
    let access = select
        .filter
        .as_ref()
        .and_then(|expr| primary_equality(expr, primary))
        .map(Access::PrimaryKey)
        .unwrap_or(Access::FullScan);
    Plan {
        access,
        select: select.clone(),
    }
}

fn primary_equality(expr: &Expr, primary: &str) -> Option<Value> {
    let Expr::Binary {
        op: BinaryOp::Eq,
        left,
        right,
    } = expr
    else {
        return None;
    };
    match (&**left, &**right) {
        (Expr::Column(column), Expr::Literal(value)) if column == primary => Some(value.clone()),
        (Expr::Literal(value), Expr::Column(column)) if column == primary => Some(value.clone()),
        _ => None,
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.access {
            Access::FullScan => write!(formatter, "Scan(table={})", self.select.table)?,
            Access::PrimaryKey(value) => write!(
                formatter,
                "IndexLookup(table={}, primary_key={value})",
                self.select.table
            )?,
        }
        if self.select.filter.is_some() {
            write!(formatter, " -> Filter")?;
        }
        write!(formatter, " -> Project")?;
        if let Some((column, ascending)) = &self.select.order_by {
            write!(
                formatter,
                " -> Sort({column} {})",
                if *ascending { "ASC" } else { "DESC" }
            )?;
        }
        if let Some(limit) = self.select.limit {
            write!(formatter, " -> Limit({limit})")?;
        }
        Ok(())
    }
}
