//! Executable obligations corresponding to lattice-formal Merkle model properties.
//! These test the implementation; Lean proofs do not constitute a refinement proof.
use lattice_cashew::{cbor, Header, MerkleDictionary, Transform};
use std::collections::BTreeMap;
fn root(t: &MerkleDictionary<u64>) -> String {
    Header::new(t.clone()).unwrap().raw_cid
}
fn build(xs: &[(&str, u64)]) -> MerkleDictionary<u64> {
    xs.iter().fold(MerkleDictionary::default(), |t, (k, v)| {
        t.inserting(k, *v).unwrap()
    })
}
#[test]
fn lean_batch_nonvacuity_examples() {
    let cases = [
        (
            vec![("ab", 1), ("x", 2)],
            vec![("ab", Transform::Update(5)), ("abc", Transform::Insert(7))],
        ),
        (
            vec![("ab", 1), ("ac", 2)],
            vec![
                ("a", Transform::Insert(3)),
                ("ab", Transform::Delete),
                ("ad", Transform::Insert(4)),
            ],
        ),
        (
            vec![("ab", 1), ("ac", 2), ("ad", 3)],
            vec![("ac", Transform::Delete), ("ad", Transform::Delete)],
        ),
    ];
    for (entries, ops) in cases {
        let start = build(&entries);
        let batch = ops
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        let final_batch = start.transform(&batch).unwrap();
        let forward = ops
            .iter()
            .fold(start.clone(), |t, (k, v)| t.apply(k, v.clone()).unwrap());
        let reverse = ops
            .iter()
            .rev()
            .fold(start, |t, (k, v)| t.apply(k, v.clone()).unwrap());
        assert_eq!(root(&final_batch), root(&forward));
        assert_eq!(root(&forward), root(&reverse));
        assert_eq!(final_batch.count, final_batch.entries().unwrap().len());
    }
}
#[test]
fn canonical_map_and_sparse_obligations() {
    let mut t = MerkleDictionary::default();
    let mut map = BTreeMap::new();
    for i in 0..250u64 {
        let key = format!("key{}", i % 37);
        if i % 3 == 0 {
            if map.remove(&key).is_some() {
                t = t.deleting(&key).unwrap()
            }
        } else if map.insert(key.clone(), i).is_some() {
            t = t.updating(&key, i).unwrap()
        } else {
            t = t.inserting(&key, i).unwrap()
        }
        assert_eq!(t.entries().unwrap(), map);
        assert_eq!(t.count, map.len());
        let rebuilt = map
            .iter()
            .rev()
            .fold(MerkleDictionary::default(), |t, (k, v)| {
                t.inserting(k, *v).unwrap()
            });
        assert_eq!(root(&t), root(&rebuilt));
        let p = t.proof(&key).unwrap();
        assert_eq!(root(&p), root(&t));
        assert_eq!(p.get(&key).unwrap(), map.get(&key));
    }
}
#[test]
fn reject_duplicate_and_reordered_children() {
    let h = Header::new(1u64).unwrap();
    #[derive(serde::Serialize)]
    struct Bad {
        count: usize,
        children: Vec<lattice_cashew::radix::Entry<Header<u64>>>,
    }
    for keys in [vec!["a", "a"], vec!["b", "a"], vec!["é", "é"], vec!["abc"]] {
        let bytes = cbor::encode(&Bad {
            count: keys.len(),
            children: keys
                .into_iter()
                .map(|k| lattice_cashew::radix::Entry {
                    key: k.into(),
                    value: h.clone(),
                })
                .collect(),
        })
        .unwrap();
        assert!(cbor::decode::<MerkleDictionary<u64>>(&bytes).is_err());
    }
}
#[test]
fn untrusted_sparse_read_verification() {
    let t = build(&[("cat", 1), ("car", 2), ("dog", 3)]);
    let digest = root(&t);
    let p = t.proof("cat").unwrap();
    let mut entries = p.sparse_entries().unwrap();
    assert_eq!(
        MerkleDictionary::<u64>::verify_read(&digest, "cat", &entries).unwrap(),
        Some(1)
    );
    assert!(MerkleDictionary::<u64>::verify_read(&digest, "dog", &entries).is_err());
    entries.get_mut(&digest).unwrap().push(0);
    assert!(MerkleDictionary::<u64>::verify_read(&digest, "cat", &entries).is_err());
}
#[test]
fn materialized_node_commitment_mismatch_is_rejected() {
    let mut t = build(&[("cat", 1)]);
    let expected = root(&t);
    std::sync::Arc::make_mut(t.children[0].value.node.as_mut().unwrap()).value = Some(999);
    assert_eq!(root(&t), expected); // parent still commits to original child CID
    assert!(t.get("cat").is_err());
    assert!(t.updating("cat", 2).is_err());
    let volume = lattice_cashew::VolumeMerkleDictionary::from_ordinary(&t);
    assert!(volume.get("cat").is_err());
    let mut h = Header::new(1u64).unwrap();
    h.node = Some(std::sync::Arc::new(2));
    assert!(h.resolve(&BTreeMap::new(), None).is_err());
}
#[test]
fn typed_headers_reject_weak_or_truncated_hashes() {
    use multihash_codetable::{Code, MultihashDigest};
    let bytes = cbor::encode(&1u64).unwrap();
    let full = Code::Sha2_256.digest(&bytes);
    let short = cid::multihash::Multihash::<64>::wrap(0x12, &full.digest()[..1]).unwrap();
    let weak = cid::Cid::new_v1(0x71, short).to_string();
    lattice_cashew::validate_entry(&weak, &bytes).unwrap();
    assert!(Header::<u64>::from_cid(&weak)
        .unwrap()
        .resolve(&BTreeMap::from([(weak, bytes)]), None)
        .is_err());
}
