use super::*;
use crate::{
    CastleFightBuilderRace, CastleFightMatch, CastleFightMatchConfig, CastleFightParticipantConfig,
    MapVersion, create_castle_fight_match,
};

const SECOND: usize = CASTLE_FIGHT_SIMULATION_HZ as usize;
const ORB: u8 = 6;
const RUNE: u8 = 0;
const CHEESE: u8 = 5;
const DOUBLE_DAMAGE: u8 = 8;

fn item(code: &str) -> ItemId {
    ItemId(u32::from_be_bytes(code.as_bytes().try_into().unwrap()))
}

fn new_match() -> CastleFightMatch {
    new_match_with(&[(0, 0), (6, 1)])
}

fn new_pair_match() -> CastleFightMatch {
    new_match_with(&[(0, 0), (1, 0), (6, 1), (7, 1)])
}

fn new_match_with(players: &[(u8, u8)]) -> CastleFightMatch {
    let participants = players
        .iter()
        .map(|&(id, team)| CastleFightParticipantConfig {
            id: PlayerId(id),
            team: Team(team),
            builder_race: CastleFightBuilderRace::Human,
        })
        .collect();
    let config = CastleFightMatchConfig::development_subset_with_participants(
        MapVersion::CASTLE_FIGHT_9_27,
        "r1",
        0x4954_454d,
        participants,
    )
    .unwrap();
    create_castle_fight_match(config, 1).unwrap()
}

fn steps(sim: &mut Simulation, count: usize) {
    for _ in 0..count {
        sim.step();
    }
}

fn builder_of(sim: &Simulation, player: u8) -> BuilderView {
    sim.builder_for_player(PlayerId(player)).unwrap()
}

fn entity_of(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|entity| entity.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}

fn spawn_dummy(sim: &mut Simulation, team: Team, at: SimPoint, health: i32) -> SimId {
    sim.spawn_unit(UnitSpawn {
        team,
        position: at,
        health,
        attack: AttackProfile {
            damage: 0,
            range: 0,
            acquisition_range: 0,
            cooldown_ticks: 1000,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    })
}

fn offset(point: SimPoint, world_units: i32) -> SimPoint {
    SimPoint::new(point.x + world_to_subunits(world_units), point.y)
}

fn grant(sim: &mut Simulation, player: u8, gold: u32, lumber: u32) {
    assert!(sim.debug_grant_player_resources_for(PlayerId(player), gold, lumber));
}

fn carry(sim: &mut Simulation, builder: SimId, code: &str, charges: u8) {
    let catalog = sim.item_catalog().unwrap();
    let entity = entity_of(sim, builder);
    sim.give_item(catalog, entity, item(code), charges).unwrap();
}

fn modifiers(sim: &Simulation, unit: SimId) -> Vec<TimedArmorModifier> {
    let status = sim.unit(unit).unwrap().status;
    status.armor_modifiers[..usize::from(status.armor_modifier_count)].to_vec()
}

#[test]
fn shop_stock_opens_after_its_delay_and_restocks_toward_the_maximum() {
    let mut game = new_pair_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0).id;
    grant(sim, 0, 5000, 0);
    sim.step();

    assert_eq!(
        sim.buy_item_as(PlayerId(0), builder, RUNE),
        Err(ItemCommandError::ShopClosed)
    );
    sim.buy_item_as(PlayerId(0), builder, ORB).unwrap();
    assert_eq!(
        sim.buy_item_as(PlayerId(0), builder, ORB),
        Err(ItemCommandError::OutOfStock)
    );
    assert_eq!(builder_of(sim, 0).inventory.count(item("I006")), 1);
    // The shop is the team's: a teammate cannot buy the single orb either.
    let teammate = builder_of(sim, 1).id;
    grant(sim, 1, 5000, 0);
    assert_eq!(
        sim.buy_item_as(PlayerId(1), teammate, ORB),
        Err(ItemCommandError::OutOfStock)
    );

    steps(sim, 30 * SECOND);
    sim.buy_item_as(PlayerId(0), builder, RUNE).unwrap();
    assert_eq!(
        sim.buy_item_as(PlayerId(0), builder, RUNE),
        Err(ItemCommandError::OutOfStock)
    );

    steps(sim, 150 * SECOND);
    sim.buy_item_as(PlayerId(1), teammate, ORB).unwrap();
}

#[test]
fn bought_item_needs_room_and_funds_before_anything_is_charged() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0).id;
    sim.step();
    let gold = sim.player_resources_for(PlayerId(0)).unwrap().gold;
    assert!(matches!(
        sim.buy_item_as(PlayerId(0), builder, ORB),
        Err(ItemCommandError::InsufficientGold { .. })
    ));
    grant(sim, 0, 5000, 0);
    for _ in 0..6 {
        carry(sim, builder, "I003", 0);
    }
    let before = sim.player_resources_for(PlayerId(0)).unwrap().gold;
    assert_eq!(
        sim.buy_item_as(PlayerId(0), builder, ORB),
        Err(ItemCommandError::InventoryFull)
    );
    assert_eq!(sim.player_resources_for(PlayerId(0)).unwrap().gold, before);
    assert!(before > gold);
    // A refused purchase does not consume stock either.
    sim.drop_item_as(PlayerId(0), builder, 0, builder_of(sim, 0).position)
        .unwrap();
    sim.buy_item_as(PlayerId(0), builder, ORB).unwrap();
}

#[test]
fn powerups_are_consumed_on_purchase_and_never_take_a_slot() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    let ally = spawn_dummy(sim, Team(0), builder.position, 1000);
    let enemy = spawn_dummy(sim, Team(1), builder.position, 1000);
    grant(sim, 0, 10_000, 0);
    steps(sim, 300 * SECOND + 1);
    let cap = sim
        .player_resources_for(PlayerId(0))
        .unwrap()
        .legendary_points_cap;

    sim.buy_item_as(PlayerId(0), builder.id, CHEESE).unwrap();
    sim.buy_item_as(PlayerId(0), builder.id, DOUBLE_DAMAGE)
        .unwrap();
    assert!(builder_of(sim, 0).inventory.is_empty());
    assert_eq!(
        sim.player_resources_for(PlayerId(0))
            .unwrap()
            .legendary_points_cap,
        cap + 1
    );

    sim.step();
    let bonus = |sim: &Simulation, unit| {
        modifiers(sim, unit)
            .iter()
            .map(|modifier| i32::from(modifier.damage_bonus_per_10k))
            .sum::<i32>()
    };
    assert_eq!(bonus(sim, ally), 10_000);
    assert_eq!(bonus(sim, enemy), 0);
    steps(sim, 29 * SECOND);
    assert_eq!(bonus(sim, ally), 0);
}

#[test]
fn carried_auras_reach_nearby_friends_only_while_carried() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    let near = spawn_dummy(sim, Team(0), offset(builder.position, 300), 1000);
    let far = spawn_dummy(sim, Team(0), offset(builder.position, 900), 1000);
    let enemy = spawn_dummy(sim, Team(1), offset(builder.position, 300), 1000);
    for unit in [near, far, enemy] {
        sim.world
            .get_mut::<Health>(entity_of(sim, unit))
            .unwrap()
            .current = 500;
    }
    carry(sim, builder.id, "I009", 0);
    carry(sim, builder.id, "I003", 0);
    steps(sim, 3 * SECOND);

    let hp = |sim: &Simulation, unit| sim.unit(unit).unwrap().health;
    assert!(hp(sim, near) > 500, "rune heals friends inside its radius");
    assert_eq!(hp(sim, far), 500);
    assert_eq!(hp(sim, enemy), 500);
    // The drums reach the whole map, structures excepted.
    assert!(
        modifiers(sim, far)
            .iter()
            .any(|m| m.damage_bonus_per_10k == 3300)
    );
    assert!(modifiers(sim, enemy).is_empty());

    for slot in 0..2 {
        sim.drop_item_as(PlayerId(0), builder.id, slot, builder.position)
            .unwrap();
    }
    steps(sim, 2);
    assert!(modifiers(sim, far).is_empty());
}

#[test]
fn drop_walk_and_pickup_move_items_through_the_world() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    carry(sim, builder.id, "I003", 0);
    let spot = offset(builder.position, 600);

    sim.drop_item_as(PlayerId(0), builder.id, 0, spot).unwrap();
    assert!(
        sim.ground_items().is_empty(),
        "the builder is still walking"
    );
    assert!(builder_of(sim, 0).item_order.is_some());
    steps(sim, 10 * SECOND);
    let [(ground, dropped)] = sim.ground_items()[..] else {
        panic!("the drop completes once the builder arrives");
    };
    assert_eq!(dropped.item, item("I003"));
    assert!(builder_of(sim, 0).inventory.is_empty());
    assert!(builder_of(sim, 0).item_order.is_none());

    sim.pickup_item_as(PlayerId(0), builder.id, ground).unwrap();
    steps(sim, 2);
    assert!(sim.ground_items().is_empty());
    assert_eq!(builder_of(sim, 0).inventory.count(item("I003")), 1);
}

#[test]
fn a_movement_command_abandons_a_pending_item_walk() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    carry(sim, builder.id, "I003", 0);
    sim.drop_item_as(PlayerId(0), builder.id, 0, offset(builder.position, 900))
        .unwrap();
    sim.order_builder_move_as(PlayerId(0), builder.id, offset(builder.position, -100))
        .unwrap();
    assert!(builder_of(sim, 0).item_order.is_none());
    steps(sim, 10 * SECOND);
    assert!(sim.ground_items().is_empty());
    assert_eq!(builder_of(sim, 0).inventory.count(item("I003")), 1);
}

#[test]
fn giving_to_a_full_ally_keeps_the_item_with_the_giver() {
    let mut game = new_pair_match();
    let sim = &mut game.simulation;
    let giver = builder_of(sim, 0);
    let receiver = builder_of(sim, 1);
    carry(sim, giver.id, "I006", 0);
    for _ in 0..6 {
        carry(sim, receiver.id, "I003", 0);
    }
    sim.give_item_as(PlayerId(0), giver.id, 0, receiver.id)
        .unwrap();
    steps(sim, 30 * SECOND);
    assert_eq!(builder_of(sim, 0).inventory.count(item("I006")), 1);

    sim.drop_item_as(PlayerId(1), receiver.id, 0, builder_of(sim, 1).position)
        .unwrap();
    sim.give_item_as(PlayerId(0), giver.id, 0, receiver.id)
        .unwrap();
    steps(sim, 30 * SECOND);
    assert_eq!(builder_of(sim, 0).inventory.count(item("I006")), 0);
    assert_eq!(builder_of(sim, 1).inventory.count(item("I006")), 1);
}

#[test]
fn point_scrolls_affect_friendly_units_in_the_area_and_spend_a_charge() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    let centre = builder.position;
    let inside = spawn_dummy(sim, Team(0), offset(centre, 400), 1000);
    let outside = spawn_dummy(sim, Team(0), offset(centre, 1500), 1000);
    sim.world
        .get_mut::<Health>(entity_of(sim, inside))
        .unwrap()
        .current = 300;
    carry(sim, builder.id, "I005", 2);
    carry(sim, builder.id, "I00F", 1);

    sim.use_item_as(PlayerId(0), builder.id, 0, ItemUseTarget::Point(centre))
        .unwrap();
    sim.use_item_as(PlayerId(0), builder.id, 1, ItemUseTarget::Point(centre))
        .unwrap();
    sim.step();

    assert!(sim.unit(inside).unwrap().health >= 700);
    assert!(
        modifiers(sim, inside)
            .iter()
            .any(|m| m.armor_bonus_per_100 == 800)
    );
    assert!(sim.unit(inside).unwrap().status.movement_modifier_count > 0);
    assert!(modifiers(sim, outside).is_empty());
    assert_eq!(sim.unit(outside).unwrap().status.movement_modifier_count, 0);
    let inventory = builder_of(sim, 0).inventory;
    assert_eq!(inventory.slot(0).unwrap().charges, 1);
    assert!(
        inventory.slot(1).is_none(),
        "a perishable scroll leaves with its last charge"
    );
}

#[test]
fn blast_staves_fire_on_their_cooldown_and_a_full_set_fires_four_missiles() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    let target = spawn_dummy(sim, Team(1), offset(builder.position, 400), 100_000);
    for _ in 0..4 {
        carry(sim, builder.id, "I001", 0);
    }
    let inventory = builder_of(sim, 0).inventory;
    assert_eq!(inventory.count(item("I002")), 1);
    assert_eq!(inventory.count(item("I001")), 0);

    for _ in 0..3 * SECOND {
        sim.step();
        if !sim.items.impacts.is_empty() {
            break;
        }
    }
    // Four independent missiles launch together and land one flight later.
    assert_eq!(sim.items.impacts.len(), 4);
    // Each missile in flight is presented as its own guaranteed-hit projectile carrying the
    // item ability, and disappears once it has landed.
    let missiles: Vec<_> = sim
        .projectiles()
        .into_iter()
        .filter(|projectile| projectile.ability == Some(crate::AbilityId(0x4130_3244)))
        .collect();
    assert_eq!(missiles.len(), 4);
    assert!(
        missiles
            .iter()
            .all(|missile| missile.kind == ProjectileViewKind::GuaranteedHit { target })
    );
    let distinct: std::collections::BTreeSet<_> = missiles.iter().map(|m| m.id).collect();
    assert_eq!(distinct.len(), 4);
    steps(sim, 2 * SECOND);
    assert!(sim.items.impacts.iter().all(|i| !distinct.contains(&i.id)));
    let health = sim.unit(target).unwrap().health;
    assert!(health < 100_000);
    steps(sim, SECOND);
    assert!(
        sim.unit(target).unwrap().health < health,
        "the staff keeps firing each second"
    );
}

#[test]
fn orb_of_lightning_levels_with_round_time_and_shares_one_cooldown() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    let target = spawn_dummy(sim, Team(1), offset(builder.position, 600), 100_000);
    carry(sim, builder.id, "I006", 0);
    sim.next_tick = 11 * 60 * CASTLE_FIGHT_SIMULATION_HZ as u64;

    sim.use_item_as(PlayerId(0), builder.id, 0, ItemUseTarget::Unit(target))
        .unwrap();
    let chains: Vec<_> = sim
        .world
        .iter_entities()
        .filter_map(|entity| {
            entity
                .get::<crate::components::ChainLightningState>()
                .copied()
        })
        .collect();
    assert_eq!(chains.len(), 1);
    assert_eq!(
        chains[0].profile.maximum_targets, 8,
        "level 3 after eleven minutes"
    );
    assert_eq!(
        sim.use_item_as(PlayerId(0), builder.id, 0, ItemUseTarget::Unit(target)),
        Err(ItemCommandError::OnCooldown)
    );
    let ally = spawn_dummy(sim, Team(0), builder.position, 10);
    assert_eq!(
        sim.use_item_as(PlayerId(0), builder.id, 0, ItemUseTarget::Unit(ally)),
        Err(ItemCommandError::OnCooldown)
    );
}

#[test]
fn item_state_survives_snapshot_restore_and_stays_in_lockstep() {
    let mut game = new_match();
    let sim = &mut game.simulation;
    let builder = builder_of(sim, 0);
    grant(sim, 0, 5000, 0);
    sim.step();
    sim.buy_item_as(PlayerId(0), builder.id, ORB).unwrap();
    carry(sim, builder.id, "I003", 0);
    sim.drop_item_as(PlayerId(0), builder.id, 1, builder.position)
        .unwrap();
    steps(sim, 301 * SECOND);
    sim.buy_item_as(PlayerId(0), builder.id, DOUBLE_DAMAGE)
        .unwrap();
    sim.step();
    assert!(!sim.ground_items().is_empty());
    assert!(!sim.team_item_buffs().is_empty());

    let snapshot = sim.capture_snapshot();
    let mut restored = new_match().simulation;
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(restored.checksum(), sim.checksum());
    assert_eq!(
        builder_of(&restored, 0).inventory,
        builder_of(sim, 0).inventory
    );
    assert_eq!(restored.ground_items(), sim.ground_items());
    for _ in 0..SECOND {
        sim.step();
        restored.step();
        assert_eq!(restored.checksum(), sim.checksum());
    }
}
