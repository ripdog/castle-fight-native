use super::*;

#[test]
fn projection_commitment_ignores_formatting_and_object_key_order_but_retains_source_identity() {
    let first = r#"{
        "fields": {"synthetic": {"speed": 1234, "range": 42}},
        "cleanse": {"building_unit_id": 1, "carrier_effect_ability_id": 2, "removed_persistent_ability_ids": [3]},
        "native_buff_ids": [4],
        "source_identity": "source-one"
    }"#;
    let reordered = r#"{"source_identity":"source-one","native_buff_ids":[4],"cleanse":{"removed_persistent_ability_ids":[3],"carrier_effect_ability_id":2,"building_unit_id":1},"fields":{"synthetic":{"range":42,"speed":1234}}}"#;
    let canonical = |text: &str| {
        serde_json::to_vec(&serde_json::from_str::<Projection>(text).unwrap()).unwrap()
    };
    assert_eq!(canonical(first), canonical(reordered));
    assert_ne!(
        canonical(first),
        canonical(&reordered.replace("source-one", "source-two"))
    );
}
