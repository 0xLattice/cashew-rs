use lattice_cashew::{
    platform::{expect_header, expect_volume},
    *,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    title: Header<String>,
    entries: Volume<MerkleDictionary<Header<String>>>,
}
impl DagNode for Record {
    fn links(&self) -> Vec<&dyn DagHeader> {
        vec![&self.title, &self.entries]
    }
}
impl PlatformNode for Record {
    fn named_children(&self) -> Result<NamedHeaders, Error> {
        Ok(BTreeMap::from([
            (
                "title".into(),
                Box::new(self.title.clone()) as Box<dyn ErasedHeader>,
            ),
            ("entries".into(), Box::new(self.entries.clone())),
        ]))
    }
    fn with_named_children(&self, mut c: NamedHeaders) -> Result<Self, Error> {
        let title = c
            .remove("title")
            .ok_or_else(|| Error::InvalidKey("title".into()))?;
        let entries = c
            .remove("entries")
            .ok_or_else(|| Error::InvalidKey("entries".into()))?;
        if !c.is_empty() {
            return Err(Error::TypeMismatch("unknown property".into()));
        }
        Ok(Self {
            title: expect_header(title.as_ref())?,
            entries: expect_volume(entries.as_ref())?,
        })
    }
}
fn plan<V>(path: &[&str], value: V) -> ArrayTrie<V> {
    let mut p = ArrayTrie::new();
    p.set(
        &path.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        value,
    );
    p
}
fn sample() -> Header<Record> {
    Header::new(Record {
        title: Header::new("Title".into()).unwrap(),
        entries: Volume::new(
            MerkleDictionary::default()
                .inserting("alice", Header::new("Alice".into()).unwrap())
                .unwrap()
                .inserting("bob", Header::new("Bob".into()).unwrap())
                .unwrap(),
        )
        .unwrap(),
    })
    .unwrap()
}
#[test]
fn heterogeneous_lazy_resolution_preserves_root_and_boundaries() {
    let h = sample();
    let data: BTreeMap<_, _> = volume::serialize_recursive(&h, &())
        .unwrap()
        .into_iter()
        .flat_map(|v| v.entries)
        .collect();
    let p = plan(&["entries", "alice"], ResolutionStrategy::Targeted);
    let out = h.removing_node().resolve_paths(&p, &data, &()).unwrap();
    assert_eq!(out.cid(), h.raw_cid);
    let c = out.named_children(&()).unwrap();
    assert!(c["entries"].is_volume());
    assert!(!c["title"].is_materialized());
    let entries = expect_volume::<MerkleDictionary<Header<String>>>(c["entries"].as_ref()).unwrap();
    let n = entries.node.as_ref().unwrap();
    assert_eq!(
        n.get("alice").unwrap().unwrap().node.as_deref(),
        Some(&"Alice".to_owned())
    );
    assert!(matches!(n.get("bob"), Err(Error::MissingContent(_))));
    assert!(expect_header::<MerkleDictionary<Header<String>>>(c["entries"].as_ref()).is_err());
}
#[test]
fn heterogeneous_transform_encryption_and_authentication() {
    let h = sample();
    let p = plan(&["title"], Transform::Update("\"New title\"".into()));
    let changed = h.transform_paths(&p, &()).unwrap().unwrap();
    assert_ne!(changed.cid(), h.raw_cid);
    let c = changed.named_children(&()).unwrap();
    assert_eq!(
        expect_header::<String>(c["title"].as_ref())
            .unwrap()
            .node
            .as_deref(),
        Some(&"New title".into())
    );
    let key = [9; 32];
    let e = changed
        .encrypt_paths(&plan(&["entries"], EncryptionStrategy::Recursive(key)), &())
        .unwrap();
    let keys = BTreeMap::from([(EncryptionInfo::new(&key, &[0; 12]).key_hash, key)]);
    let c = e.named_children(&keys).unwrap();
    assert!(c["entries"].encryption_info().is_some());
    assert!(c["entries"].is_volume());
    assert!(c["entries"].named_children(&()).is_err());
    assert_eq!(c["entries"].named_children(&keys).unwrap().len(), 2);
    let entries = expect_volume::<MerkleDictionary<Header<String>>>(c["entries"].as_ref()).unwrap();
    let stored = volume::serialize_recursive(&entries, &keys).unwrap();
    let data = stored
        .into_iter()
        .flat_map(|v| v.entries)
        .collect::<BTreeMap<_, _>>();
    let resolved = entries
        .removing_node()
        .resolve_paths(&plan(&[], ResolutionStrategy::Recursive), &data, &keys)
        .unwrap();
    assert_eq!(resolved.cid(), entries.raw_cid);
}
#[test]
fn sparse_proof_nested_and_collapse() {
    let t = MerkleDictionary::default()
        .inserting("ab", Header::new("B".to_owned()).unwrap())
        .unwrap()
        .inserting("ac", Header::new("C".to_owned()).unwrap())
        .unwrap()
        .inserting("ad", Header::new("D".to_owned()).unwrap())
        .unwrap()
        .inserting("unrelated", Header::new("U".to_owned()).unwrap())
        .unwrap();
    let h = Header::new(t.clone()).unwrap();
    let data = volume::serialize_recursive(&h, &())
        .unwrap()
        .into_iter()
        .flat_map(|v| v.entries)
        .collect::<BTreeMap<_, _>>();
    let mut p = plan(&["ac"], ProofKind::Deletion);
    p.set(&["ad"], ProofKind::Deletion);
    let witness = PlatformNode::proof_paths(&t, &p, &data, &()).unwrap();
    let ops = BTreeMap::from([
        ("ac".into(), Transform::Delete),
        ("ad".into(), Transform::Delete),
    ]);
    assert_eq!(
        Header::new(witness.transform(&ops).unwrap())
            .unwrap()
            .raw_cid,
        Header::new(t.transform(&ops).unwrap()).unwrap().raw_cid
    );
    assert!(witness.get("unrelated").is_err());
    assert_eq!(Header::new(witness).unwrap().raw_cid, h.raw_cid);
}
#[test]
fn range_is_bounded_and_lazy() {
    let t = MerkleDictionary::default()
        .inserting("aa", 1u64)
        .unwrap()
        .inserting("bb", 2)
        .unwrap()
        .inserting("cc", 3)
        .unwrap();
    let data = t.serialize_volume().unwrap().entries;
    let lazy = cbor::decode_value(&cbor::encode(&t).unwrap()).unwrap();
    let lazy: MerkleDictionary<u64> = cbor::decode(&cbor::encode_value(&lazy).unwrap()).unwrap();
    let p = plan(
        &[],
        ResolutionStrategy::Range {
            after: Some("aa".into()),
            limit: 1,
        },
    );
    let out = PlatformNode::resolve_paths(&lazy, &p, &data, &()).unwrap();
    assert_eq!(out.get("bb").unwrap(), Some(&2));
    // Cursor branch may be loaded to establish its exclusion.
    assert_eq!(out.get("aa").unwrap(), Some(&1));
    assert!(out.get("cc").is_err());
}

#[test]
fn owned_walk_preserves_edge_multiplicity_and_postorder() {
    let leaf = Header::new("shared".to_owned()).unwrap();
    let root = Header::new(BTreeMap::from([
        ("a".to_owned(), leaf.clone()),
        ("b".to_owned(), leaf.clone()),
    ]))
    .unwrap();
    let mut seen = Default::default();
    let mut result = vec![];
    volume::walk_owned_subtree(&root, &mut seen, &(), &mut |cid, edges| {
        result.push((cid.to_owned(), edges.clone()))
    })
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0, leaf.raw_cid);
    assert_eq!(result[1].1[&leaf.raw_cid], 2);
    volume::walk_owned_subtree(&root, &mut seen, &(), &mut |_, _| panic!("already visited"))
        .unwrap();
}
#[tokio::test]
async fn storage_session_retries_errors_and_deduplicates_concurrent_writers() {
    use lattice_cashew::source::{VolumeStorageSession, VolumeStorer};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Store {
        calls: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl VolumeStorer for Store {
        async fn store_volume(&self, _: SerializedVolume) -> Result<(), Error> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if n == 0 {
                Err(Error::Storage("retry".into()))
            } else {
                Ok(())
            }
        }
    }
    let store = Arc::new(Store { calls: 0.into() });
    let session = VolumeStorageSession::new(store.clone());
    let h = Volume::new("hello".to_owned()).unwrap();
    let v = volume::serialize_boundary(&h, &()).unwrap();
    assert!(session.store_volume(v.clone()).await.is_err());
    let (a, b) = tokio::join!(
        session.store_volume(v.clone()),
        session.store_volume(v.clone())
    );
    a.unwrap();
    b.unwrap();
    session.store_volume(v).await.unwrap();
    assert_eq!(store.calls.load(Ordering::SeqCst), 2);
    assert!(session.is_stored(&h.raw_cid).await);
}
#[tokio::test]
async fn asynchronous_heterogeneous_hydration_and_proof() {
    let h = sample();
    let data = volume::serialize_recursive(&h, &())
        .unwrap()
        .into_iter()
        .flat_map(|v| v.entries)
        .collect::<BTreeMap<_, _>>();
    let p = plan(&["entries", "alice"], ResolutionStrategy::Targeted);
    let out = h
        .removing_node()
        .resolve_paths_async(&p, &data, &())
        .await
        .unwrap();
    assert_eq!(out.raw_cid, h.raw_cid);
    let c = out.named_children(&()).unwrap();
    assert!(!c["title"].is_materialized());
    let p = plan(&["entries", "alice"], ProofKind::Existence);
    let out = h
        .removing_node()
        .proof_paths_async(&p, &data, &())
        .await
        .unwrap();
    assert_eq!(out.raw_cid, h.raw_cid);
}
#[test]
fn selected_storage_keeps_internal_volume_boundaries_and_omits_other_leaf_volumes() {
    let a = Volume::new("Alice".to_owned()).unwrap();
    let b = Volume::new("Bob".to_owned()).unwrap();
    let t = VolumeMerkleDictionary::default()
        .inserting("alice", a.clone())
        .unwrap()
        .inserting("bob", b.clone())
        .unwrap();
    let h = Volume::new(t).unwrap();
    let selected =
        volume::serialize_selected(&h, &plan(&["alice"], StorageStrategy::Targeted), &()).unwrap();
    assert_eq!(selected.len(), 3);
    assert!(selected.iter().any(|v| v.root == h.raw_cid));
    assert!(selected.iter().any(|v| v.root == a.raw_cid));
    assert!(!selected.iter().any(|v| v.root == b.raw_cid));
    for v in selected {
        assert_eq!(v.entries.len(), 1);
        v.validate().unwrap()
    }
    let all = volume::serialize_selected(&h, &plan(&[], StorageStrategy::Recursive), &()).unwrap();
    assert_eq!(all.len(), 5);
}
#[test]
fn recursive_storage_skips_unmaterialized_nested_volume() {
    let a = Volume::new("Alice".to_owned()).unwrap();
    let b = Volume::new("Bob".to_owned()).unwrap().removing_node();
    let h = Volume::new(BTreeMap::from([
        ("a".to_owned(), a.clone()),
        ("b".to_owned(), b),
    ]))
    .unwrap();
    let all = volume::serialize_recursive(&h, &()).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|v| v.root == a.raw_cid));
}
#[test]
fn array_delete_swaps_last_and_nested_query_dispatches() {
    let a = MerkleArray::default()
        .append(1u64)
        .unwrap()
        .append(2)
        .unwrap()
        .append(3)
        .unwrap();
    let b = a.deleting_at(0).unwrap();
    assert_eq!(b.0.count, 2);
    assert_eq!(b.get(0).unwrap(), Some(&3));
    assert_eq!(b.get(1).unwrap(), Some(&2));
    assert!(b.deleting_at(2).is_err());
    let inner = MerkleDictionary::default().inserting("x", 4u64).unwrap();
    let a = MerkleArray::default()
        .append(Header::new(inner).unwrap())
        .unwrap();
    let p = query::Plan::compile(&query::parse("first | get 'x'").unwrap());
    assert_eq!(
        a.execute(&p).unwrap().1,
        query::QueryResult::Value(Some("4".into()))
    );
}
