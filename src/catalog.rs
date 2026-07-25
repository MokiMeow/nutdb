//! Persisted table schemas and row encoding for the SQL layer.

use std::io;

use crate::sql::ast::{ColumnDef, DataType, Value};
use crate::txn::{Transaction, TxnError};

const CATALOG_PREFIX: &str = "__CATALOG:";
const ROW_PREFIX: &str = "__ROW:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

impl Schema {
    pub fn validate(&self) -> io::Result<()> {
        if self.columns.is_empty() {
            return Err(input("catalog: table needs at least one column"));
        }
        let primary = self
            .columns
            .iter()
            .filter(|column| column.primary_key)
            .count();
        if primary != 1 {
            return Err(input("catalog: table needs exactly one primary key"));
        }
        for (index, column) in self.columns.iter().enumerate() {
            if self.columns[..index]
                .iter()
                .any(|earlier| earlier.name == column.name)
            {
                return Err(input("catalog: duplicate column"));
            }
        }
        Ok(())
    }

    pub fn primary_index(&self) -> usize {
        self.columns
            .iter()
            .position(|column| column.primary_key)
            .expect("validated schema")
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name == name)
    }
}

pub fn load_schema(txn: &Transaction, table: &str) -> io::Result<Schema> {
    let encoded = txn
        .get(&catalog_key(table))
        .map_err(txn_error)?
        .ok_or_else(|| input(&format!("sql: unknown table '{table}'")))?;
    decode_schema(table, &encoded)
}

pub fn save_schema(txn: &mut Transaction, schema: &Schema) -> io::Result<()> {
    schema.validate()?;
    txn.set(catalog_key(&schema.name), encode_schema(schema))
        .map_err(txn_error)
}

pub fn catalog_key(table: &str) -> String {
    format!("{CATALOG_PREFIX}{table}")
}

pub fn row_prefix(table: &str) -> String {
    format!("{ROW_PREFIX}{table}:")
}

pub fn row_key(schema: &Schema, row: &[Value]) -> io::Result<String> {
    let primary = row
        .get(schema.primary_index())
        .ok_or_else(|| input("sql: row does not match schema"))?;
    if *primary == Value::Null {
        return Err(input("sql: primary key cannot be NULL"));
    }
    row_key_for_primary(schema, primary)
}

pub fn row_key_for_primary(schema: &Schema, primary: &Value) -> io::Result<String> {
    if *primary == Value::Null {
        return Err(input("sql: primary key cannot be NULL"));
    }
    check_type(primary, &schema.columns[schema.primary_index()].data_type)?;
    Ok(format!(
        "{}{}",
        row_prefix(&schema.name),
        hex(&encode_value(primary))
    ))
}

pub fn encode_row(row: &[Value]) -> String {
    row.iter()
        .map(|value| hex(&encode_value(value)))
        .collect::<Vec<_>>()
        .join(":")
}

pub fn decode_row(encoded: &str, schema: &Schema) -> io::Result<Vec<Value>> {
    let values: io::Result<Vec<Value>> = encoded
        .split(':')
        .map(|part| decode_value(&unhex(part)?))
        .collect();
    let values = values?;
    if values.len() != schema.columns.len() {
        return Err(invalid("catalog: row column count mismatch"));
    }
    for (value, column) in values.iter().zip(&schema.columns) {
        check_type(value, &column.data_type)?;
    }
    Ok(values)
}

pub fn check_type(value: &Value, data_type: &DataType) -> io::Result<()> {
    if *value == Value::Null {
        return Ok(());
    }
    let matches = matches!(
        (value, data_type),
        (Value::Integer(_), DataType::Integer)
            | (Value::Text(_), DataType::Text)
            | (Value::Boolean(_), DataType::Boolean)
    );
    if matches {
        Ok(())
    } else {
        Err(input("sql: value has the wrong column type"))
    }
}

fn encode_schema(schema: &Schema) -> String {
    schema
        .columns
        .iter()
        .map(|column| {
            let kind = match column.data_type {
                DataType::Integer => "I",
                DataType::Text => "T",
                DataType::Boolean => "B",
            };
            format!(
                "{}|{}|{}",
                column.name,
                kind,
                if column.primary_key { "1" } else { "0" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_schema(name: &str, encoded: &str) -> io::Result<Schema> {
    let mut columns = Vec::new();
    for line in encoded.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() != 3 {
            return Err(invalid("catalog: malformed schema"));
        }
        let data_type = match fields[1] {
            "I" => DataType::Integer,
            "T" => DataType::Text,
            "B" => DataType::Boolean,
            _ => return Err(invalid("catalog: unknown column type")),
        };
        let primary_key = match fields[2] {
            "0" => false,
            "1" => true,
            _ => return Err(invalid("catalog: malformed primary-key flag")),
        };
        columns.push(ColumnDef {
            name: fields[0].to_owned(),
            data_type,
            primary_key,
        });
    }
    let schema = Schema {
        name: name.to_owned(),
        columns,
    };
    schema.validate()?;
    Ok(schema)
}

fn encode_value(value: &Value) -> Vec<u8> {
    match value {
        Value::Null => vec![b'N'],
        Value::Integer(value) => {
            let mut out = vec![b'I'];
            out.extend_from_slice(&value.to_le_bytes());
            out
        }
        Value::Text(value) => {
            let mut out = vec![b'T'];
            out.extend_from_slice(value.as_bytes());
            out
        }
        Value::Boolean(value) => vec![b'B', u8::from(*value)],
    }
}

fn decode_value(bytes: &[u8]) -> io::Result<Value> {
    match bytes.first() {
        Some(b'N') if bytes.len() == 1 => Ok(Value::Null),
        Some(b'I') if bytes.len() == 9 => Ok(Value::Integer(i64::from_le_bytes(
            bytes[1..9].try_into().expect("fixed slice"),
        ))),
        Some(b'T') => Ok(Value::Text(
            std::str::from_utf8(&bytes[1..])
                .map_err(|_| invalid("catalog: row text is not UTF-8"))?
                .to_owned(),
        )),
        Some(b'B') if bytes.len() == 2 && bytes[1] <= 1 => Ok(Value::Boolean(bytes[1] == 1)),
        _ => Err(invalid("catalog: malformed row value")),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn unhex(value: &str) -> io::Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        return Err(invalid("catalog: odd hex length"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = digit(pair[0])?;
            let low = digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn digit(byte: u8) -> io::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(invalid("catalog: invalid hex")),
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

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
