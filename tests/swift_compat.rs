use lattice_cashew::{cbor, header, Header, MerkleDictionary};
use serde::Serialize;
fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Serialize)]
struct Record {
    name: String,
    value: i64,
    #[serde(with = "cbor::base64_bytes")]
    bytes: Vec<u8>,
}
#[test]
fn swift_generated_vectors() {
    let fixtures = include_str!("swift.tsv")
        .lines()
        .map(|s| s.split_once('\t').unwrap())
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(hex(&cbor::encode(&u64::MAX).unwrap()), fixtures["int"]);
    assert_eq!(hex(&cbor::encode(&i64::MIN).unwrap()), fixtures["negative"]);
    assert_eq!(hex(&cbor::encode(&1.5f64).unwrap()), fixtures["float"]);
    assert_eq!(
        hex(&cbor::encode(&Record {
            name: "test".into(),
            value: 23,
            bytes: vec![0, 1, 255]
        })
        .unwrap()),
        fixtures["record"]
    );
    for n in 0..=32 {
        let mut dictionary = MerkleDictionary::default();
        for i in 0..n {
            let key = format!(
                "{}{}",
                ["cat", "car", "cab", "c", "dog", "🐱", "🐱x", "é", "éx"][i % 9],
                i / 9
            );
            dictionary = dictionary.inserting(&key, i).unwrap();
        }
        assert_eq!(
            hex(&cbor::encode(&dictionary).unwrap()),
            fixtures[format!("dictionary-{n}").as_str()],
            "dictionary-{n}"
        );
        assert_eq!(
            Header::new(dictionary).unwrap().raw_cid,
            fixtures[format!("cid-{n}").as_str()],
            "cid-{n}"
        );
    }
    assert_eq!(
        hex(&header::encrypt(&[0, 1, 2, 3], &[7; 32], &[8; 12]).unwrap()),
        fixtures["encrypted"]
    );
}
