//! Production observations only: no model transform is implemented here.
use lattice_cashew::{
    verify_content_address, Header, MerkleDictionary, RadixNode, Transform, VolumeMerkleDictionary,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
type Tree = MerkleDictionary<u64>;
fn chars(s: &str) -> Vec<u8> {
    assert!(s.bytes().all(|b| (0x21..=0x7e).contains(&b)));
    s.bytes().collect()
}
fn child_key(s: &str) -> u8 {
    let c = chars(s);
    assert_eq!(c.len(), 1);
    c[0]
}
fn record(cid: &str, bytes: Vec<u8>, enc: Value, table: &mut BTreeMap<String, Value>) {
    verify_content_address(cid, &bytes).unwrap();
    let id = enc.to_string();
    let row = json!({"enc":enc,"cid":cid,"bytes":bytes});
    if let Some(old) = table.insert(id, row.clone()) {
        assert_eq!(old, row);
    }
}
fn node(h: &Header<RadixNode<u64>>, table: &mut BTreeMap<String, Value>) -> Value {
    if let Some(n) = &h.node {
        let kids: Vec<_> = n
            .children
            .iter()
            .map(|e| json!([child_key(&e.key), node(&e.value, table)]))
            .collect();
        record(
            &h.raw_cid,
            h.bytes(None).unwrap(),
            json!({"node":[chars(&n.prefix),n.value,n.children.iter().map(|e|json!([child_key(&e.key),e.value.raw_cid])).collect::<Vec<_>>()]}),
            table,
        );
        json!({"node":[chars(&n.prefix),n.value,kids]})
    } else {
        json!({"stub":h.raw_cid})
    }
}
fn root(t: &Tree, table: &mut BTreeMap<String, Value>) -> Value {
    let kids: Vec<_> = t
        .children
        .iter()
        .map(|e| json!([child_key(&e.key), node(&e.value, table)]))
        .collect();
    let h = Header::new(t.clone()).unwrap();
    record(
        &h.raw_cid,
        h.bytes(None).unwrap(),
        json!({"root":[t.count,t.children.iter().map(|e|json!([child_key(&e.key),e.value.raw_cid])).collect::<Vec<_>>()]}),
        table,
    );
    json!({"count":t.count,"kids":kids})
}
fn observe(
    name: String,
    before: Tree,
    ops: BTreeMap<String, Transform<u64>>,
    table: &mut BTreeMap<String, Value>,
) -> Value {
    let cid = Header::new(before.clone()).unwrap().raw_cid;
    let source = before.serialize_volume().unwrap().entries;
    let after = before.transform(&ops).ok();
    let proof = before.prove_transforms(&ops, &source).ok();
    let partial_after = proof.as_ref().and_then(|p| p.transform(&ops).ok());
    let mut stubbed = before.clone();
    for e in &mut stubbed.children {
        e.value = e.value.removing_node();
    }
    let stubbed_after = stubbed.transform(&ops).ok();
    let volume = VolumeMerkleDictionary::from_ordinary(&before);
    let volume_after = volume.transform(&ops).ok();
    assert_eq!(
        after
            .as_ref()
            .map(|t| Header::new(t.clone()).unwrap().raw_cid),
        volume_after.map(|t| Header::new(t).unwrap().raw_cid)
    );
    assert_eq!(Header::new(before.clone()).unwrap().raw_cid, cid);
    let queries: Vec<_> = ["", "a", "ab", "abc", "ac", "b", "ba", "z", "zz"]
        .iter()
        .map(|k| json!({"key":chars(k),"value":after.as_ref().map(|t|t.get(k).unwrap().copied())}))
        .collect();
    json!({"name":name,"before":root(&before,table),"beforeCID":cid,"after":after.as_ref().map(|t|root(t,table)),"afterCID":after.as_ref().map(|t|Header::new(t.clone()).unwrap().raw_cid),"proof":proof.as_ref().map(|t|root(t,table)),"partialAfter":partial_after.as_ref().map(|t|root(t,table)),"stubbed":root(&stubbed,table),"stubbedAfter":stubbed_after.as_ref().map(|t|root(t,table)),"queries":queries,"ops":ops.iter().map(|(k,o)|match o{Transform::Insert(v)=>json!([chars(k),"insert",v]),Transform::Update(v)=>json!([chars(k),"update",v]),Transform::Delete=>json!([chars(k),"delete"])}).collect::<Vec<_>>()})
}
fn main() {
    let mut rows = vec![];
    let mut table = BTreeMap::new();
    let keys = ["a", "ab", "abc", "ac", "z"];
    for mask in 0..32 {
        let mut before = Tree::default();
        for (i, k) in keys.iter().enumerate() {
            if mask & (1 << i) != 0 {
                before = before.inserting(k, (i + 1) as u64).unwrap();
            }
        }
        let batches: Vec<Vec<(&str, Transform<u64>)>> = vec![
            vec![],
            vec![("a", Transform::Update(9)), ("ab", Transform::Insert(10))],
            vec![
                ("a", Transform::Insert(11)),
                ("ab", Transform::Delete),
                ("ac", Transform::Update(12)),
            ],
            vec![("ab", Transform::Delete), ("ac", Transform::Delete)],
            vec![("a", Transform::Delete), ("abc", Transform::Delete)],
            vec![("ba", Transform::Insert(13)), ("zz", Transform::Insert(14))],
            vec![
                ("abc", Transform::Update(u64::MAX)),
                ("z", Transform::Update(0)),
            ],
            vec![("", Transform::Insert(1))],
            vec![("zz", Transform::Delete)],
            vec![("ab", Transform::Insert(3))],
            vec![("a", Transform::Insert(4)), ("z", Transform::Delete)],
            vec![
                ("a", Transform::Delete),
                ("ab", Transform::Delete),
                ("abc", Transform::Delete),
                ("ac", Transform::Delete),
                ("z", Transform::Delete),
            ],
        ];
        for (i, batch) in batches.into_iter().enumerate() {
            rows.push(observe(
                format!("state{mask}-batch{i}"),
                before.clone(),
                batch.into_iter().map(|(k, v)| (k.into(), v)).collect(),
                &mut table,
            ));
        }
    }
    let branch = Tree::default()
        .inserting("ab", 1)
        .unwrap()
        .inserting("ac", 2)
        .unwrap();
    rows.push(observe(
        "delete-valueless-branch".into(),
        branch,
        BTreeMap::from([("a".into(), Transform::Delete)]),
        &mut table,
    ));
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"cases":rows,"encodings":table.values().collect::<Vec<_>>()})
        )
        .unwrap()
    );
}
