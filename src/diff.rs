use crate::{Error, Header, MerkleDictionary};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CashewDiff {
    pub inserted: BTreeMap<String, String>,
    pub deleted: BTreeMap<String, String>,
    pub modified: BTreeMap<String, ModifiedEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModifiedEntry {
    pub old: String,
    pub new: String,
    pub children: CashewDiff,
}
impl CashewDiff {
    pub fn is_empty(&self) -> bool {
        self.inserted.is_empty() && self.deleted.is_empty() && self.modified.is_empty()
    }
    pub fn change_count(&self) -> usize {
        self.inserted.len()
            + self.deleted.len()
            + self
                .modified
                .values()
                .map(|e| e.children.change_count().max(1))
                .sum::<usize>()
    }
    fn render(&self, depth: usize, lines: &mut Vec<String>) {
        let pad = "  ".repeat(depth);
        for (k, v) in &self.inserted {
            lines.push(format!("{pad}+ {k}: {v}"))
        }
        for (k, v) in &self.deleted {
            lines.push(format!("{pad}- {k}: {v}"))
        }
        for (k, v) in &self.modified {
            if v.children.is_empty() {
                lines.push(format!("{pad}~ {k}: {} → {}", v.old, v.new))
            } else {
                lines.push(format!("{pad}~ {k}:"));
                v.children.render(depth + 1, lines)
            }
        }
    }
}
impl std::fmt::Display for CashewDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut lines = vec![];
        self.render(0, &mut lines);
        write!(f, "{}", lines.join("\n"))
    }
}
impl<V: Clone + Serialize + ToString + PartialEq> MerkleDictionary<V> {
    pub fn diff(&self, other: &Self) -> Result<CashewDiff, Error> {
        if Header::new(self.clone())?.raw_cid == Header::new(other.clone())?.raw_cid {
            return Ok(CashewDiff::default());
        }
        let a = self.entries()?;
        let b = other.entries()?;
        let mut diff = CashewDiff::default();
        for (k, v) in &a {
            match b.get(k) {
                None => {
                    diff.deleted.insert(k.clone(), v.to_string());
                }
                Some(w) if v != w => {
                    diff.modified.insert(
                        k.clone(),
                        ModifiedEntry {
                            old: v.to_string(),
                            new: w.to_string(),
                            children: CashewDiff::default(),
                        },
                    );
                }
                _ => {}
            }
        }
        for (k, v) in b {
            if !a.contains_key(&k) {
                diff.inserted.insert(k, v.to_string());
            }
        }
        Ok(diff)
    }
}

/// Compare materialized heterogeneous nodes. Missing child bodies produce a shallow
/// CID modification; present bodies are authenticated before recursively comparing.
pub fn diff_headers(
    old: &dyn crate::ErasedHeader,
    new: &dyn crate::ErasedHeader,
    keys: &dyn crate::KeyProvider,
) -> Result<CashewDiff, Error> {
    if old.cid() == new.cid() {
        return Ok(CashewDiff::default());
    }
    if !old.is_materialized() || !new.is_materialized() {
        return Ok(CashewDiff::default());
    }
    let a = old.named_children(keys)?;
    let b = new.named_children(keys)?;
    let mut out = CashewDiff::default();
    for (k, h) in &a {
        match b.get(k) {
            None => {
                out.deleted.insert(k.clone(), h.cid().into());
            }
            Some(n) if h.cid() != n.cid() => {
                out.modified.insert(
                    k.clone(),
                    ModifiedEntry {
                        old: h.cid().into(),
                        new: n.cid().into(),
                        children: diff_headers(h.as_ref(), n.as_ref(), keys)?,
                    },
                );
            }
            _ => {}
        }
    }
    for (k, h) in b {
        if !a.contains_key(&k) {
            out.inserted.insert(k, h.cid().into());
        }
    }
    Ok(out)
}
