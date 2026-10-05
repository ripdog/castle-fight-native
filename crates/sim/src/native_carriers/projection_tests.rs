use super::*;

#[test]
fn hostile_masks_require_supported_relations_and_never_discard_qualifiers() {
    let mask = hostile_native_targets("structure, ground, enemies").unwrap();
    assert!(mask.can_target_unit(crate::MovementClass::Ground));
    assert!(mask.can_target_buildings());
    assert!(!mask.can_target_unit(crate::MovementClass::Air));
    for mask in [
        "ground",
        "enemy",
        "",
        "ground,friend",
        "ground,self,enemy",
        "ground,organic,enemy",
        "ground,vulnerable,enemy",
        "ground,hero,enemy",
        "ground,ground,enemy",
        "ground,enemy,enemies",
        "ground,enemy,",
    ] {
        assert!(hostile_native_targets(mask).is_err(), "{mask}");
    }
}

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
