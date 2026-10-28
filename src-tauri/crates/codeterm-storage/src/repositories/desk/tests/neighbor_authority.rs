use super::*;
use serde_json::json;

#[test]
fn neighbor_authority_survives_validated_desk_save_and_reopen_for_roots_and_descendants() {
    for root_tab_id in [None, Some("root-concierge")] {
        let (root, storage) = storage();
        storage
            .desk()
            .import(desk())
            .wait(Duration::from_secs(5))
            .unwrap();
        let before = storage
            .desk()
            .snapshot()
            .wait(Duration::from_secs(5))
            .unwrap();
        let mut expected = before.value.snapshot;
        expected.workspaces[0].tabs[0].layout = DeskLayoutNode::Leaf {
            tab_id: "tab-a".into(),
            cwd: Some("C:/friend".into()),
            fields: DeskLeafFields {
                values: json!({"neighbor_authority": {
                    "uid":"opaque-owner", "friendship_id":"a".repeat(64), "root_tab_id":root_tab_id
                }}),
            },
        };
        storage
            .desk()
            .save_preserving(expected.clone(), before.value.revision)
            .wait(Duration::from_secs(5))
            .unwrap();
        storage.shutdown().wait(Duration::from_secs(5)).unwrap();
        drop(storage);
        let reopened = Storage::open(root.path(), StorageConfig::default()).unwrap();
        let actual = reopened
            .desk()
            .snapshot()
            .wait(Duration::from_secs(5))
            .unwrap();
        assert_eq!(actual.value.snapshot, expected);
    }
}

#[test]
fn neighbor_authority_leaf_field_accepts_object_or_null_and_keeps_unknown_fields_strict() {
    for value in [
        json!({"uid":"opaque-owner","friendship_id":"a".repeat(64),"root_tab_id":null}),
        json!(null),
    ] {
        assert!(DeskLeafFields {
            values: json!({"neighbor_authority":value})
        }
        .validate()
        .is_ok());
    }
    for value in [json!("wrong"), json!(true), json!(7), json!([])] {
        assert!(DeskLeafFields {
            values: json!({"neighbor_authority":value})
        }
        .validate()
        .is_err());
    }
    assert!(DeskLeafFields {
        values: json!({"neighbor_authority_typo":{}})
    }
    .validate()
    .is_err());
}
