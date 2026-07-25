//! SQL execution over [`crate::txn::MvccStore`].
//!
//! Scans and filters use a pull iterator (`next() -> Result<Option<Row>>`).
//! Sorting is the one intentionally materializing operator.

use std::cmp::Ordering;
use std::io;
use std::path::Path;

use crate::catalog::{
    check_type, decode_row, encode_row, load_schema, row_key, row_key_for_primary, row_prefix,
    save_schema, Schema,
};
use crate::txn::{MvccStore, Transaction, TxnError};

use super::ast::{
    BinaryOp, Expr, Projection, Select, SqlResult, Statement, Value,
};
use super::parser::parse;
use super::plan::{plan_select, Access, Plan};

type Row = Vec<Value>;

pub struct SqlEngine {
    store: MvccStore,
    active: Option<Transaction>,
}

impl SqlEngine {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            store: MvccStore::open(path)?,
            active: None,
        })
    }

    pub fn execute(&mut self, source: &str) -> io::Result<Vec<SqlResult>> {
        let statements = parse(source)?;
        let mut results = Vec::with_capacity(statements.len());
        for statement in statements {
            results.push(self.execute_statement(statement)?);
        }
        Ok(results)
    }

    fn execute_statement(&mut self, statement: Statement) -> io::Result<SqlResult> {
        match statement {
            Statement::Begin => {
                if self.active.is_some() {
                    return Err(input("sql: transaction already active"));
                }
                self.active = Some(self.store.begin().map_err(txn_error)?);
                Ok(SqlResult::Begun)
            }
            Statement::Commit => {
                let txn = self
                    .active
                    .take()
                    .ok_or_else(|| input("sql: no active transaction"))?;
                txn.commit().map_err(txn_error)?;
                Ok(SqlResult::Committed)
            }
            Statement::Rollback => {
                let txn = self
                    .active
                    .take()
                    .ok_or_else(|| input("sql: no active transaction"))?;
                txn.abort().map_err(txn_error)?;
                Ok(SqlResult::RolledBack)
            }
            other => self.with_transaction(|txn| execute_data(txn, other)),
        }
    }

    fn with_transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut Transaction) -> io::Result<T>,
    ) -> io::Result<T> {
        if let Some(txn) = self.active.as_mut() {
            return operation(txn);
        }
        let mut txn = self.store.begin().map_err(txn_error)?;
        match operation(&mut txn) {
            Ok(result) => {
                txn.commit().map_err(txn_error)?;
                Ok(result)
            }
            Err(error) => {
                let _ = txn.abort();
                Err(error)
            }
        }
    }
}

fn execute_data(txn: &mut Transaction, statement: Statement) -> io::Result<SqlResult> {
    match statement {
        Statement::CreateTable { table, columns } => {
            let schema = Schema {
                name: table.clone(),
                columns,
            };
            schema.validate()?;
            if txn
                .get(&crate::catalog::catalog_key(&table))
                .map_err(txn_error)?
                .is_some()
            {
                return Err(input(&format!("sql: table '{table}' already exists")));
            }
            save_schema(txn, &schema)?;
            Ok(SqlResult::Affected(0))
        }
        Statement::Insert {
            table,
            columns,
            values,
        } => insert(txn, &table, columns, values),
        Statement::Select(select) => select_rows(txn, &select),
        Statement::Update {
            table,
            assignments,
            filter,
        } => update(txn, &table, assignments, filter),
        Statement::Delete { table, filter } => delete(txn, &table, filter),
        Statement::Explain(select) => {
            let schema = load_schema(txn, &select.table)?;
            Ok(SqlResult::Explain(plan_select(&select, &schema).to_string()))
        }
        Statement::Begin | Statement::Commit | Statement::Rollback => {
            Err(input("sql: internal transaction dispatch error"))
        }
    }
}

fn insert(
    txn: &mut Transaction,
    table: &str,
    columns: Vec<String>,
    values: Vec<Value>,
) -> io::Result<SqlResult> {
    let schema = load_schema(txn, table)?;
    if columns.len() != values.len() {
        return Err(input("sql: INSERT column/value count mismatch"));
    }
    let mut row = vec![Value::Null; schema.columns.len()];
    let mut assigned = vec![false; schema.columns.len()];
    for (column, value) in columns.into_iter().zip(values) {
        let index = schema
            .column_index(&column)
            .ok_or_else(|| input(&format!("sql: unknown column '{column}'")))?;
        if assigned[index] {
            return Err(input("sql: duplicate INSERT column"));
        }
        check_type(&value, &schema.columns[index].data_type)?;
        row[index] = value;
        assigned[index] = true;
    }
    let key = row_key(&schema, &row)?;
    if txn.get(&key).map_err(txn_error)?.is_some() {
        return Err(input("sql: duplicate primary key"));
    }
    txn.set(key, encode_row(&row)).map_err(txn_error)?;
    Ok(SqlResult::Affected(1))
}

fn select_rows(txn: &Transaction, select: &Select) -> io::Result<SqlResult> {
    let schema = load_schema(txn, &select.table)?;
    let plan = plan_select(select, &schema);
    let rows = access_rows(txn, &schema, &plan)?;
    let mut source: Box<dyn RowSource> = Box::new(VecSource {
        rows: rows.into_iter(),
    });
    if let Some(filter) = &select.filter {
        source = Box::new(FilterSource {
            input: source,
            filter: filter.clone(),
            schema: schema.clone(),
        });
    }
    let mut materialized = Vec::new();
    while let Some(row) = source.next()? {
        materialized.push(row);
    }
    if let Some((column, ascending)) = &select.order_by {
        let index = schema
            .column_index(column)
            .ok_or_else(|| input(&format!("sql: unknown ORDER BY column '{column}'")))?;
        materialized.sort_by(|left, right| {
            let order = compare_for_sort(&left[index], &right[index]);
            if *ascending {
                order
            } else {
                order.reverse()
            }
        });
    }
    if let Some(limit) = select.limit {
        materialized.truncate(limit);
    }

    let (columns, indexes) = projection(&schema, &select.projection)?;
    let rows = materialized
        .into_iter()
        .map(|row| indexes.iter().map(|index| row[*index].clone()).collect())
        .collect();
    Ok(SqlResult::Rows { columns, rows })
}

fn update(
    txn: &mut Transaction,
    table: &str,
    assignments: Vec<(String, Value)>,
    filter: Option<Expr>,
) -> io::Result<SqlResult> {
    let schema = load_schema(txn, table)?;
    let rows = all_rows(txn, &schema)?;
    let mut affected = 0;
    for (old_key, mut row) in rows {
        if !matches_filter(filter.as_ref(), &row, &schema)? {
            continue;
        }
        for (column, value) in &assignments {
            let index = schema
                .column_index(column)
                .ok_or_else(|| input(&format!("sql: unknown column '{column}'")))?;
            check_type(value, &schema.columns[index].data_type)?;
            row[index] = value.clone();
        }
        let new_key = row_key(&schema, &row)?;
        if new_key != old_key {
            if txn.get(&new_key).map_err(txn_error)?.is_some() {
                return Err(input("sql: duplicate primary key"));
            }
            txn.delete(old_key).map_err(txn_error)?;
        }
        txn.set(new_key, encode_row(&row)).map_err(txn_error)?;
        affected += 1;
    }
    Ok(SqlResult::Affected(affected))
}

fn delete(
    txn: &mut Transaction,
    table: &str,
    filter: Option<Expr>,
) -> io::Result<SqlResult> {
    let schema = load_schema(txn, table)?;
    let rows = all_rows(txn, &schema)?;
    let mut affected = 0;
    for (key, row) in rows {
        if matches_filter(filter.as_ref(), &row, &schema)? {
            txn.delete(key).map_err(txn_error)?;
            affected += 1;
        }
    }
    Ok(SqlResult::Affected(affected))
}

fn access_rows(txn: &Transaction, schema: &Schema, plan: &Plan) -> io::Result<Vec<Row>> {
    match &plan.access {
        Access::FullScan => Ok(all_rows(txn, schema)?
            .into_iter()
            .map(|(_, row)| row)
            .collect()),
        Access::PrimaryKey(value) if *value == Value::Null => Ok(Vec::new()),
        Access::PrimaryKey(value) => {
            let key = row_key_for_primary(schema, value)?;
            match txn.get(&key).map_err(txn_error)? {
                Some(encoded) => Ok(vec![decode_row(&encoded, schema)?]),
                None => Ok(Vec::new()),
            }
        }
    }
}

fn all_rows(txn: &Transaction, schema: &Schema) -> io::Result<Vec<(String, Row)>> {
    txn.scan_prefix(&row_prefix(&schema.name))
        .map_err(txn_error)?
        .into_iter()
        .map(|(key, encoded)| Ok((key, decode_row(&encoded, schema)?)))
        .collect()
}

trait RowSource {
    fn next(&mut self) -> io::Result<Option<Row>>;
}

struct VecSource {
    rows: std::vec::IntoIter<Row>,
}

impl RowSource for VecSource {
    fn next(&mut self) -> io::Result<Option<Row>> {
        Ok(self.rows.next())
    }
}

struct FilterSource {
    input: Box<dyn RowSource>,
    filter: Expr,
    schema: Schema,
}

impl RowSource for FilterSource {
    fn next(&mut self) -> io::Result<Option<Row>> {
        while let Some(row) = self.input.next()? {
            if matches_filter(Some(&self.filter), &row, &self.schema)? {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }
}

fn projection(schema: &Schema, projection: &Projection) -> io::Result<(Vec<String>, Vec<usize>)> {
    match projection {
        Projection::All => Ok((
            schema.columns.iter().map(|column| column.name.clone()).collect(),
            (0..schema.columns.len()).collect(),
        )),
        Projection::Columns(columns) => {
            let indexes: io::Result<Vec<usize>> = columns
                .iter()
                .map(|column| {
                    schema
                        .column_index(column)
                        .ok_or_else(|| input(&format!("sql: unknown column '{column}'")))
                })
                .collect();
            Ok((columns.clone(), indexes?))
        }
    }
}

fn matches_filter(filter: Option<&Expr>, row: &[Value], schema: &Schema) -> io::Result<bool> {
    let Some(filter) = filter else {
        return Ok(true);
    };
    Ok(eval(filter, row, schema)? == Value::Boolean(true))
}

fn eval(expr: &Expr, row: &[Value], schema: &Schema) -> io::Result<Value> {
    match expr {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Column(column) => {
            let index = schema
                .column_index(column)
                .ok_or_else(|| input(&format!("sql: unknown column '{column}'")))?;
            Ok(row[index].clone())
        }
        Expr::Binary { op, left, right } => {
            let left = eval(left, row, schema)?;
            let right = eval(right, row, schema)?;
            match op {
                BinaryOp::And => logic_and(left, right),
                BinaryOp::Or => logic_or(left, right),
                _ => compare(*op, left, right),
            }
        }
    }
}

fn compare(op: BinaryOp, left: Value, right: Value) -> io::Result<Value> {
    if left == Value::Null || right == Value::Null {
        return Ok(Value::Null);
    }
    let order = match (&left, &right) {
        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
        (Value::Text(a), Value::Text(b)) => a.cmp(b),
        (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
        _ => return Err(input("sql: cannot compare different types")),
    };
    let result = match op {
        BinaryOp::Eq => order == Ordering::Equal,
        BinaryOp::Ne => order != Ordering::Equal,
        BinaryOp::Lt => order == Ordering::Less,
        BinaryOp::Le => order != Ordering::Greater,
        BinaryOp::Gt => order == Ordering::Greater,
        BinaryOp::Ge => order != Ordering::Less,
        BinaryOp::And | BinaryOp::Or => unreachable!("handled above"),
    };
    Ok(Value::Boolean(result))
}

fn logic_and(left: Value, right: Value) -> io::Result<Value> {
    match (truth(left)?, truth(right)?) {
        (Some(false), _) | (_, Some(false)) => Ok(Value::Boolean(false)),
        (Some(true), Some(true)) => Ok(Value::Boolean(true)),
        _ => Ok(Value::Null),
    }
}

fn logic_or(left: Value, right: Value) -> io::Result<Value> {
    match (truth(left)?, truth(right)?) {
        (Some(true), _) | (_, Some(true)) => Ok(Value::Boolean(true)),
        (Some(false), Some(false)) => Ok(Value::Boolean(false)),
        _ => Ok(Value::Null),
    }
}

fn truth(value: Value) -> io::Result<Option<bool>> {
    match value {
        Value::Boolean(value) => Ok(Some(value)),
        Value::Null => Ok(None),
        _ => Err(input("sql: AND/OR expects boolean values")),
    }
}

fn compare_for_sort(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => left.cmp(right),
    }
}

fn txn_error(error: TxnError) -> io::Error {
    match error {
        TxnError::Io(error) => error,
        other => input(&other.to_string()),
    }
}

fn input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
