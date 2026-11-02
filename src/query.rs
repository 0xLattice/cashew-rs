use crate::{Error, MerkleDictionary, Transform};
use serde::Serialize;
use std::{collections::BTreeMap, str::FromStr, sync::Arc};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expression {
    Get(String),
    GetAt(i64),
    Keys,
    SortedKeys {
        limit: Option<i64>,
        after: Option<String>,
    },
    Values,
    SortedValues {
        limit: Option<i64>,
        after: Option<String>,
    },
    Count,
    Contains(String),
    First,
    Last,
    Insert(String, String),
    Update(String, String),
    Set(String, String),
    Delete(String),
    Append(String),
}
#[derive(Clone, Debug)]
pub enum Step {
    Transform(BTreeMap<String, Transform<String>>),
    Evaluate(Expression),
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub steps: Vec<Step>,
}
#[derive(Clone, Debug)]
pub enum QueryResult {
    Value(Option<String>),
    Bool(bool),
    Count(usize),
    List(Vec<String>),
    Entries(Vec<(String, String)>),
    Node(Arc<dyn Queryable>),
    Ok,
}

/// Object-safe nested query dispatch. Custom structured nodes implement this trait.
pub trait Queryable: std::fmt::Debug {
    fn execute_query(&self, plan: &Plan) -> Result<QueryResult, Error>;
}
/// Leaf values are text-convertible; references opt into polymorphic query traversal.
pub trait QueryValue: Clone + Serialize + FromStr + ToString + std::fmt::Debug + 'static {
    fn query_node(&self) -> Option<Arc<dyn Queryable>> {
        None
    }
}
macro_rules! scalar_query {($($t:ty),*)=>{$(impl QueryValue for $t{})*}}
scalar_query!(
    String, bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64
);
impl<T: Queryable + Serialize + Clone + 'static> QueryValue for crate::Header<T> {
    fn query_node(&self) -> Option<Arc<dyn Queryable>> {
        Some(Arc::new(self.clone()))
    }
}
impl<T: Queryable + Serialize + Clone + 'static> QueryValue for crate::Volume<T> {
    fn query_node(&self) -> Option<Arc<dyn Queryable>> {
        Some(Arc::new(self.clone()))
    }
}
impl<T: Queryable + Serialize> Queryable for crate::Header<T> {
    fn execute_query(&self, plan: &Plan) -> Result<QueryResult, Error> {
        self.bytes(None)?;
        self.node
            .as_deref()
            .ok_or_else(|| Error::MissingContent(self.raw_cid.clone()))?
            .execute_query(plan)
    }
}
impl<T: Queryable + Serialize> Queryable for crate::Volume<T> {
    fn execute_query(&self, plan: &Plan) -> Result<QueryResult, Error> {
        self.0.execute_query(plan)
    }
}
impl<V: QueryValue> Queryable for MerkleDictionary<V> {
    fn execute_query(&self, plan: &Plan) -> Result<QueryResult, Error> {
        Ok(self.execute(plan)?.1)
    }
}
impl<V: QueryValue> Queryable for crate::MerkleArray<V> {
    fn execute_query(&self, plan: &Plan) -> Result<QueryResult, Error> {
        Ok(self.execute(plan)?.1)
    }
}
impl<V: QueryValue> Queryable for crate::VolumeMerkleDictionary<V> {
    fn execute_query(&self, plan: &Plan) -> Result<QueryResult, Error> {
        Ok(self.execute(plan)?.1)
    }
}
impl PartialEq for QueryResult {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Value(a), Self::Value(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Count(a), Self::Count(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Entries(a), Self::Entries(b)) => a == b,
            (Self::Ok, Self::Ok) => true,
            _ => false,
        }
    }
}

impl Plan {
    pub fn compile(expressions: &[Expression]) -> Self {
        let mut steps = vec![];
        let mut batch = BTreeMap::new();
        for e in expressions {
            let mutation = match e {
                Expression::Insert(k, v) => Some((k, Transform::Insert(v.clone()))),
                Expression::Update(k, v) => Some((k, Transform::Update(v.clone()))),
                Expression::Delete(k) => Some((k, Transform::Delete)),
                _ => None,
            };
            if let Some((k, t)) = mutation {
                if batch.contains_key(k) {
                    steps.push(Step::Transform(std::mem::take(&mut batch)))
                }
                batch.insert(k.clone(), t);
            } else {
                if !batch.is_empty() {
                    steps.push(Step::Transform(std::mem::take(&mut batch)))
                }
                steps.push(Step::Evaluate(e.clone()));
            }
        }
        if !batch.is_empty() {
            steps.push(Step::Transform(batch))
        }
        Self { steps }
    }
}
impl<V: QueryValue> MerkleDictionary<V> {
    pub fn execute(&self, plan: &Plan) -> Result<(Self, QueryResult), Error> {
        let mut node = self.clone();
        let mut result = QueryResult::Ok;
        let parse = |s: &str| {
            s.parse::<V>()
                .map_err(|_| Error::InvalidKey(format!("invalid value {s}")))
        };
        for (index, step) in plan.steps.iter().enumerate() {
            if let Step::Evaluate(Expression::Get(key)) = step {
                if let Some(child) = node.get(key)?.and_then(QueryValue::query_node) {
                    let remaining = Plan {
                        steps: plan.steps[index + 1..].to_vec(),
                    };
                    let result = if remaining.steps.is_empty() {
                        QueryResult::Node(child)
                    } else {
                        child.execute_query(&remaining)?
                    };
                    return Ok((node, result));
                }
            }
            match step {
                Step::Transform(ops) => {
                    for (k, op) in ops {
                        node = match op {
                            Transform::Insert(v) => node.inserting(k, parse(v)?)?,
                            Transform::Update(v) => node.updating(k, parse(v)?)?,
                            Transform::Delete => node.deleting(k)?,
                        };
                    }
                    result = QueryResult::Ok
                }
                Step::Evaluate(e) => {
                    result = match e {
                        Expression::Get(k) => {
                            QueryResult::Value(node.get(k)?.map(ToString::to_string))
                        }
                        Expression::Contains(k) => QueryResult::Bool(node.get(k)?.is_some()),
                        Expression::Count => QueryResult::Count(node.count),
                        Expression::Keys => {
                            QueryResult::List(node.entries()?.into_keys().collect())
                        }
                        Expression::Values => QueryResult::Entries(
                            node.entries()?
                                .into_iter()
                                .map(|(k, v)| (k, v.to_string()))
                                .collect(),
                        ),
                        Expression::SortedKeys { limit, after } => QueryResult::List(
                            node.sorted_entries(
                                after.as_deref(),
                                limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX),
                            )?
                            .into_iter()
                            .map(|(k, _)| k)
                            .collect(),
                        ),
                        Expression::SortedValues { limit, after } => QueryResult::Entries(
                            node.sorted_entries(
                                after.as_deref(),
                                limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX),
                            )?
                            .into_iter()
                            .map(|(k, v)| (k, v.to_string()))
                            .collect(),
                        ),
                        Expression::Set(k, v) => {
                            let v = parse(v)?;
                            node = if node.get(k)?.is_some() {
                                node.updating(k, v)?
                            } else {
                                node.inserting(k, v)?
                            };
                            QueryResult::Ok
                        }
                        _ => return Err(Error::Unsupported),
                    }
                }
            }
        }
        Ok((node, result))
    }
}
#[derive(Clone, Debug)]
enum Token {
    Word(String),
    Text(String),
    Number(i64),
    Equal,
    Pipe,
}
fn invalid() -> Error {
    Error::InvalidKey("invalid query expression".into())
}
pub fn parse(input: &str) -> Result<Vec<Expression>, Error> {
    let mut chars = input.chars().peekable();
    let mut tokens = vec![];
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '|' => tokens.push(Token::Pipe),
            '=' => tokens.push(Token::Equal),
            '\'' | '"' => {
                let mut s = String::new();
                let mut closed = false;
                while let Some(n) = chars.next() {
                    if n == c {
                        closed = true;
                        break;
                    }
                    if n == '\\' {
                        s.push(chars.next().ok_or_else(invalid)?)
                    } else {
                        s.push(n)
                    }
                }
                if !closed {
                    return Err(invalid());
                }
                tokens.push(Token::Text(s))
            }
            c if c.is_ascii_digit()
                || (c == '-' && chars.peek().is_some_and(char::is_ascii_digit)) =>
            {
                let mut n = c.to_string();
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    n.push(chars.next().unwrap())
                }
                tokens.push(Token::Number(n.parse().map_err(|_| invalid())?))
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut word = c.to_string();
                while chars
                    .peek()
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                {
                    word.push(chars.next().unwrap())
                }
                tokens.push(Token::Word(word.to_lowercase()))
            }
            _ => return Err(invalid()),
        }
    }
    tokens
        .split(|t| matches!(t, Token::Pipe))
        .map(parse_one)
        .collect()
}
fn parse_one(tokens: &[Token]) -> Result<Expression, Error> {
    let Some((Token::Word(command), rest)) = tokens.split_first() else {
        return Err(invalid());
    };
    let string = |i: usize| {
        if let Some(Token::Text(s)) = rest.get(i) {
            Ok(s.clone())
        } else {
            Err(invalid())
        }
    };
    Ok(match command.as_str() {
        "get" => {
            if matches!(rest.first(),Some(Token::Word(w))if w=="at") {
                let Some(Token::Number(n)) = rest.get(1) else {
                    return Err(invalid());
                };
                Expression::GetAt(*n)
            } else {
                Expression::Get(string(0)?)
            }
        }
        "keys" | "members" | "values" => {
            let values = command == "values";
            if rest.is_empty() {
                if values {
                    Expression::Values
                } else {
                    Expression::Keys
                }
            } else {
                if !matches!(rest.first(),Some(Token::Word(w))if w=="sorted") {
                    return Err(invalid());
                }
                let mut limit = None;
                let mut after = None;
                for pair in rest[1..].chunks(2) {
                    match pair {
                        [Token::Word(k), Token::Number(n)] if k == "limit" => limit = Some(*n),
                        [Token::Word(k), Token::Text(s)] if k == "after" => after = Some(s.clone()),
                        _ => return Err(invalid()),
                    }
                }
                if values {
                    Expression::SortedValues { limit, after }
                } else {
                    Expression::SortedKeys { limit, after }
                }
            }
        }
        "count" | "size" => Expression::Count,
        "first" => Expression::First,
        "last" => Expression::Last,
        "contains" | "has" => Expression::Contains(string(0)?),
        "delete" | "remove" => Expression::Delete(string(0)?),
        "append" => Expression::Append(string(0)?),
        "insert" | "add" | "update" | "set" | "put" => {
            let key = string(0)?;
            if !matches!(rest.get(1), Some(Token::Equal)) {
                return Err(invalid());
            }
            let value = string(2)?;
            match command.as_str() {
                "insert" | "add" => Expression::Insert(key, value),
                "update" => Expression::Update(key, value),
                _ => Expression::Set(key, value),
            }
        }
        _ => return Err(invalid()),
    })
}
impl<V: QueryValue> crate::MerkleArray<V> {
    pub fn execute(&self, plan: &Plan) -> Result<(Self, QueryResult), Error> {
        let mut node = self.clone();
        let mut result = QueryResult::Ok;
        for (index, step) in plan.steps.iter().enumerate() {
            let selected = match step {
                Step::Evaluate(Expression::GetAt(i)) if *i >= 0 => node.get(*i as usize)?,
                Step::Evaluate(Expression::First) => node.first()?,
                Step::Evaluate(Expression::Last) => node.last()?,
                _ => None,
            };
            if let Some(child) = selected.and_then(QueryValue::query_node) {
                return Ok((
                    node,
                    if index + 1 == plan.steps.len() {
                        QueryResult::Node(child)
                    } else {
                        child.execute_query(&Plan {
                            steps: plan.steps[index + 1..].to_vec(),
                        })?
                    },
                ));
            }
            match step {
                Step::Evaluate(Expression::GetAt(i)) => {
                    result = QueryResult::Value(if *i < 0 {
                        None
                    } else {
                        node.get(*i as usize)?.map(ToString::to_string)
                    })
                }
                Step::Evaluate(Expression::First) => {
                    result = QueryResult::Value(node.first()?.map(ToString::to_string))
                }
                Step::Evaluate(Expression::Last) => {
                    result = QueryResult::Value(node.last()?.map(ToString::to_string))
                }
                Step::Evaluate(Expression::Append(s)) => {
                    node = node.append(
                        s.parse()
                            .map_err(|_| Error::InvalidKey(format!("invalid value {s}")))?,
                    )?;
                    result = QueryResult::Ok
                }
                step => {
                    let (next, r) = node.0.execute(&Plan {
                        steps: vec![step.clone()],
                    })?;
                    node.0 = next;
                    result = r
                }
            }
        }
        Ok((node, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_key_flush_and_barriers() {
        let plan = Plan::compile(&parse("insert 'a' = '1' | update 'a' = '2' | get 'a'").unwrap());
        assert_eq!(plan.steps.len(), 3);
        let (n, r) = MerkleDictionary::<i64>::default().execute(&plan).unwrap();
        assert_eq!(n.get("a").unwrap(), Some(&2));
        assert_eq!(r, QueryResult::Value(Some("2".into())));
    }
    #[test]
    fn parser_quoted_pipe() {
        assert_eq!(
            parse("get 'a|b' | values sorted limit 2 after 'a'").unwrap(),
            vec![
                Expression::Get("a|b".into()),
                Expression::SortedValues {
                    limit: Some(2),
                    after: Some("a".into())
                }
            ]
        );
        for s in ["", "get a", "get 'x' |", "get 'x"] {
            assert!(parse(s).is_err())
        }
    }
}

impl<H: QueryValue> Queryable for BTreeMap<String, H> {
    fn execute_query(&self, plan: &Plan) -> Result<QueryResult, Error> {
        let mut node = self.clone();
        let mut result = QueryResult::Ok;
        for (index, step) in plan.steps.iter().enumerate() {
            match step {
                Step::Transform(ops) => {
                    for (k, op) in ops {
                        match op {
                            Transform::Insert(s) => {
                                if node.contains_key(k) {
                                    return Err(Error::InvalidKey(k.clone()));
                                }
                                node.insert(
                                    k.clone(),
                                    s.parse().map_err(|_| Error::InvalidKey(s.clone()))?,
                                );
                            }
                            Transform::Update(s) => {
                                if !node.contains_key(k) {
                                    return Err(Error::InvalidKey(k.clone()));
                                }
                                node.insert(
                                    k.clone(),
                                    s.parse().map_err(|_| Error::InvalidKey(s.clone()))?,
                                );
                            }
                            Transform::Delete => {
                                if node.remove(k).is_none() {
                                    return Err(Error::InvalidKey(k.clone()));
                                }
                            }
                        }
                    }
                    result = QueryResult::Ok
                }
                Step::Evaluate(e) => {
                    result = match e {
                        Expression::Get(k) => {
                            if let Some(child) = node.get(k).and_then(QueryValue::query_node) {
                                let remaining = Plan {
                                    steps: plan.steps[index + 1..].to_vec(),
                                };
                                return if remaining.steps.is_empty() {
                                    Ok(QueryResult::Node(child))
                                } else {
                                    child.execute_query(&remaining)
                                };
                            }
                            QueryResult::Value(node.get(k).map(ToString::to_string))
                        }
                        Expression::Count => QueryResult::Count(node.len()),
                        Expression::Keys => QueryResult::List(node.keys().cloned().collect()),
                        Expression::Contains(k) => QueryResult::Bool(node.contains_key(k)),
                        Expression::SortedKeys { limit, after } => QueryResult::List(
                            node.keys()
                                .filter(|k| after.as_ref().is_none_or(|a| *k > a))
                                .take(limit.map(|i| i.max(0) as usize).unwrap_or(usize::MAX))
                                .cloned()
                                .collect(),
                        ),
                        Expression::Set(k, s) => {
                            node.insert(
                                k.clone(),
                                s.parse().map_err(|_| Error::InvalidKey(s.clone()))?,
                            );
                            QueryResult::Ok
                        }
                        _ => return Err(Error::Unsupported),
                    }
                }
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod nested_tests {
    use super::*;
    #[test]
    fn typed_nested_header_and_volume_queries() {
        let accounts = MerkleDictionary::default()
            .inserting("alice", 7u64)
            .unwrap();
        let root = MerkleDictionary::default()
            .inserting("accounts", crate::Header::new(accounts).unwrap())
            .unwrap();
        let plan = Plan::compile(&parse("get 'accounts' | get 'alice'").unwrap());
        assert_eq!(
            root.execute(&plan).unwrap().1,
            QueryResult::Value(Some("7".into()))
        );
        let map = BTreeMap::from([("root".into(), crate::Volume::new(root).unwrap())]);
        let plan = Plan::compile(&parse("get 'root' | get 'accounts' | count").unwrap());
        assert_eq!(map.execute_query(&plan).unwrap(), QueryResult::Count(1));
    }
    #[test]
    fn nested_mutation_result_does_not_rewrite_parent() {
        let child = MerkleDictionary::default()
            .inserting("alice", 7u64)
            .unwrap();
        let root = MerkleDictionary::default()
            .inserting("accounts", crate::Header::new(child).unwrap())
            .unwrap();
        let before = crate::Header::new(root.clone()).unwrap().raw_cid;
        let plan =
            Plan::compile(&parse("get 'accounts' | update 'alice' = '9' | get 'alice'").unwrap());
        let (parent, result) = root.execute(&plan).unwrap();
        assert_eq!(result, QueryResult::Value(Some("9".into())));
        assert_eq!(crate::Header::new(parent).unwrap().raw_cid, before);
    }
}
