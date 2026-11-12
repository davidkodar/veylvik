use std::collections::BTreeSet;
use veylvik_document::*;

#[test]
fn targeted_inspection_pages_large_graphs_without_copying_animation_payloads() {
    let mut doc = blank(64, 32, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.clear();
    doc.output = NodeId::from_u128(1);
    for index in 1..=80u128 {
        let mut node = Node::new(
            "Noise",
            Operation::Noise {
                seed: index as u32,
                scale: 8.0.into(),
            },
            &[],
        );
        node.id = NodeId::from_u128(index);
        doc.nodes.insert(node.id, node);
    }
    let first = doc.nodes.get_mut(&doc.output).unwrap();
    first.label = "å".repeat(300);
    first.operation = Operation::Noise {
        seed: 1,
        scale: Scalar::Smooth {
            keys: (0..4096)
                .map(|index| Key {
                    time: Time::new(index, 24).unwrap(),
                    value: 1.0 + (index % 7) as f32,
                })
                .collect(),
        },
    };
    doc.validate().unwrap();
    let before = serde_json::to_vec(&doc).unwrap();
    let summary = inspection::summary(&doc);
    assert_eq!(summary["node_count"], 80);
    assert_eq!(summary["operations"]["noise"], 80);
    assert!(summary.get("nodes").is_none());
    assert!(summary.to_string().len() < 1024);
    let mut seen = BTreeSet::new();
    let mut offset = 0;
    loop {
        let page = inspection::page(&doc, offset, 7).unwrap();
        assert_eq!(page["total"], 80);
        assert!(page.to_string().len() < 4096);
        for node in page["nodes"].as_array().unwrap() {
            assert!(seen.insert(node["id"].as_str().unwrap().to_owned()));
            assert!(node.get("operation").is_none());
        }
        if offset == 0 {
            assert_eq!(
                page["nodes"][0]["label"].as_str().unwrap().chars().count(),
                256
            );
            assert_eq!(page["nodes"][0]["label_truncated"], true);
        }
        let Some(next) = page["next_offset"].as_u64() else {
            break;
        };
        offset = next;
    }
    assert_eq!(seen.len(), 80);
    assert_eq!(
        inspection::page(&doc, 80, 32).unwrap()["nodes"],
        serde_json::json!([])
    );
    for (offset, limit) in [(81, 32), (0, 0), (0, 129), (u64::MAX, 1)] {
        assert!(inspection::page(&doc, offset, limit).is_err());
    }
    let full = inspection::node(&doc, doc.output, None).unwrap();
    assert_eq!(
        full["node"],
        serde_json::to_value(&doc.nodes[&doc.output]).unwrap()
    );
    let parameter = inspection::node(&doc, doc.output, Some("scale")).unwrap();
    assert!(parameter.get("node").is_none());
    assert_eq!(parameter["value"], full["node"]["operation"]["scale"]);
    assert!(inspection::node(&doc, doc.output, Some("unknown")).is_err());
    assert!(inspection::node(&doc, NodeId::nil(), None).is_err());
    assert_eq!(serde_json::to_vec(&doc).unwrap(), before);
}
