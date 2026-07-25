//! Recursive-descent SQL parser.

use std::io;

use super::ast::{
    BinaryOp, ColumnDef, DataType, Expr, Projection, Select, Statement, Value,
};
use super::lexer::{error, lex, Token, TokenKind};

pub fn parse(source: &str) -> io::Result<Vec<Statement>> {
    let mut parser = Parser {
        tokens: lex(source)?,
        offset: 0,
    };
    let mut statements = Vec::new();
    while !parser.at_end() {
        if parser.consume_simple(&TokenKind::Semicolon) {
            continue;
        }
        statements.push(parser.statement()?);
        if !parser.consume_simple(&TokenKind::Semicolon) && !parser.at_end() {
            return Err(parser.expected("';' between statements"));
        }
    }
    if statements.is_empty() {
        return Err(error(0, "expected a statement"));
    }
    Ok(statements)
}

struct Parser {
    tokens: Vec<Token>,
    offset: usize,
}

impl Parser {
    fn statement(&mut self) -> io::Result<Statement> {
        if self.consume_word("CREATE") {
            self.expect_word("TABLE")?;
            return self.create_table();
        }
        if self.consume_word("INSERT") {
            self.expect_word("INTO")?;
            return self.insert();
        }
        if self.consume_word("SELECT") {
            return Ok(Statement::Select(self.select_after_keyword()?));
        }
        if self.consume_word("UPDATE") {
            return self.update();
        }
        if self.consume_word("DELETE") {
            self.expect_word("FROM")?;
            return self.delete();
        }
        if self.consume_word("BEGIN") {
            return Ok(Statement::Begin);
        }
        if self.consume_word("COMMIT") {
            return Ok(Statement::Commit);
        }
        if self.consume_word("ROLLBACK") {
            return Ok(Statement::Rollback);
        }
        if self.consume_word("EXPLAIN") {
            self.expect_word("SELECT")?;
            return Ok(Statement::Explain(self.select_after_keyword()?));
        }
        Err(self.expected("CREATE, INSERT, SELECT, UPDATE, DELETE, or transaction command"))
    }

    fn create_table(&mut self) -> io::Result<Statement> {
        let table = self.identifier()?;
        self.expect_simple(TokenKind::LeftParen, "'('")?;
        let mut columns = Vec::new();
        loop {
            let name = self.identifier()?;
            let type_name = self.word()?;
            let data_type = match type_name.as_str() {
                "INT" | "INTEGER" => DataType::Integer,
                "TEXT" => DataType::Text,
                "BOOL" | "BOOLEAN" => DataType::Boolean,
                _ => return Err(self.expected("INT, TEXT, or BOOLEAN")),
            };
            let primary_key = if self.consume_word("PRIMARY") {
                self.expect_word("KEY")?;
                true
            } else {
                false
            };
            columns.push(ColumnDef {
                name,
                data_type,
                primary_key,
            });
            if !self.consume_simple(&TokenKind::Comma) {
                break;
            }
        }
        self.expect_simple(TokenKind::RightParen, "')'")?;
        Ok(Statement::CreateTable { table, columns })
    }

    fn insert(&mut self) -> io::Result<Statement> {
        let table = self.identifier()?;
        self.expect_simple(TokenKind::LeftParen, "'('")?;
        let columns = self.identifier_list(TokenKind::RightParen)?;
        self.expect_word("VALUES")?;
        self.expect_simple(TokenKind::LeftParen, "'('")?;
        let mut values = Vec::new();
        loop {
            values.push(self.literal()?);
            if !self.consume_simple(&TokenKind::Comma) {
                break;
            }
        }
        self.expect_simple(TokenKind::RightParen, "')'")?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn select_after_keyword(&mut self) -> io::Result<Select> {
        let projection = if self.consume_simple(&TokenKind::Star) {
            Projection::All
        } else {
            Projection::Columns(self.identifier_list_until_word("FROM")?)
        };
        self.expect_word("FROM")?;
        let table = self.identifier()?;
        let filter = if self.consume_word("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        let order_by = if self.consume_word("ORDER") {
            self.expect_word("BY")?;
            let column = self.identifier()?;
            let ascending = if self.consume_word("DESC") {
                false
            } else {
                self.consume_word("ASC");
                true
            };
            Some((column, ascending))
        } else {
            None
        };
        let limit = if self.consume_word("LIMIT") {
            let token = self.advance().clone();
            match token.kind {
                TokenKind::Integer(value) if value >= 0 => Some(value as usize),
                _ => return Err(error(token.pos, "LIMIT expects a nonnegative integer")),
            }
        } else {
            None
        };
        Ok(Select {
            table,
            projection,
            filter,
            order_by,
            limit,
        })
    }

    fn update(&mut self) -> io::Result<Statement> {
        let table = self.identifier()?;
        self.expect_word("SET")?;
        let mut assignments = Vec::new();
        loop {
            let column = self.identifier()?;
            self.expect_simple(TokenKind::Eq, "'='")?;
            assignments.push((column, self.literal()?));
            if !self.consume_simple(&TokenKind::Comma) {
                break;
            }
        }
        let filter = if self.consume_word("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assignments,
            filter,
        })
    }

    fn delete(&mut self) -> io::Result<Statement> {
        let table = self.identifier()?;
        let filter = if self.consume_word("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Statement::Delete { table, filter })
    }

    fn expr(&mut self) -> io::Result<Expr> {
        self.or()
    }

    fn or(&mut self) -> io::Result<Expr> {
        let mut expr = self.and()?;
        while self.consume_word("OR") {
            expr = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(expr),
                right: Box::new(self.and()?),
            };
        }
        Ok(expr)
    }

    fn and(&mut self) -> io::Result<Expr> {
        let mut expr = self.comparison()?;
        while self.consume_word("AND") {
            expr = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(expr),
                right: Box::new(self.comparison()?),
            };
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> io::Result<Expr> {
        let left = self.primary()?;
        let op = match self.peek().kind {
            TokenKind::Eq => Some(BinaryOp::Eq),
            TokenKind::Ne => Some(BinaryOp::Ne),
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::Le => Some(BinaryOp::Le),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::Ge => Some(BinaryOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.offset += 1;
            Ok(Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(self.primary()?),
            })
        } else {
            Ok(left)
        }
    }

    fn primary(&mut self) -> io::Result<Expr> {
        if self.consume_simple(&TokenKind::LeftParen) {
            let expr = self.expr()?;
            self.expect_simple(TokenKind::RightParen, "')'")?;
            return Ok(expr);
        }
        match self.peek().kind.clone() {
            TokenKind::Integer(_) | TokenKind::String(_) => {
                Ok(Expr::Literal(self.literal()?))
            }
            TokenKind::Word(ref word)
                if matches!(word.as_str(), "NULL" | "TRUE" | "FALSE") =>
            {
                Ok(Expr::Literal(self.literal()?))
            }
            TokenKind::Word(_) => Ok(Expr::Column(self.identifier()?)),
            _ => Err(self.expected("column, literal, or parenthesized expression")),
        }
    }

    fn literal(&mut self) -> io::Result<Value> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Integer(value) => Ok(Value::Integer(value)),
            TokenKind::String(value) => Ok(Value::Text(value)),
            TokenKind::Word(word) if word == "NULL" => Ok(Value::Null),
            TokenKind::Word(word) if word == "TRUE" => Ok(Value::Boolean(true)),
            TokenKind::Word(word) if word == "FALSE" => Ok(Value::Boolean(false)),
            _ => Err(error(token.pos, "expected a literal value")),
        }
    }

    fn identifier_list(&mut self, end: TokenKind) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        loop {
            names.push(self.identifier()?);
            if self.consume_simple(&end) {
                break;
            }
            self.expect_simple(TokenKind::Comma, "','")?;
        }
        Ok(names)
    }

    fn identifier_list_until_word(&mut self, word: &str) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        loop {
            names.push(self.identifier()?);
            if self.check_word(word) {
                break;
            }
            self.expect_simple(TokenKind::Comma, "','")?;
        }
        Ok(names)
    }

    fn identifier(&mut self) -> io::Result<String> {
        self.word()
    }

    fn word(&mut self) -> io::Result<String> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Word(word) => Ok(word),
            _ => Err(error(token.pos, "expected an identifier")),
        }
    }

    fn expect_word(&mut self, word: &str) -> io::Result<()> {
        if self.consume_word(word) {
            Ok(())
        } else {
            Err(self.expected(&format!("'{word}'")))
        }
    }

    fn consume_word(&mut self, word: &str) -> bool {
        if self.check_word(word) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn check_word(&self, word: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Word(candidate) if candidate == word)
    }

    fn expect_simple(&mut self, kind: TokenKind, description: &str) -> io::Result<()> {
        if self.consume_simple(&kind) {
            Ok(())
        } else {
            Err(self.expected(description))
        }
    }

    fn consume_simple(&mut self, kind: &TokenKind) -> bool {
        if &self.peek().kind == kind {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn expected(&self, expected: &str) -> io::Error {
        error(self.peek().pos, &format!("expected {expected}"))
    }

    fn advance(&mut self) -> &Token {
        let index = self.offset;
        if !self.at_end() {
            self.offset += 1;
        }
        &self.tokens[index]
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.offset]
    }

    fn at_end(&self) -> bool {
        self.peek().kind == TokenKind::End
    }
}
