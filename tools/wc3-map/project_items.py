#!/usr/bin/env python3
"""Project the Castle Fight item system (castle shop, inventory rules, item effects).

Every number is read from the extracted resolved tables, which are authoritative. The
retained script is only asserted against: the shop order, the buy/pickup control flow and
the dummy-caster proxies must still match what this projection encodes.
"""
import argparse
import csv
from decimal import Decimal
import hashlib
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[2]
RESOLVED = ROOT / 'docs/original_map/extracted/resolved'
csv.field_size_limit(10**9)
parser = argparse.ArgumentParser()
parser.add_argument('--source', type=pathlib.Path, required=True)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
source = args.source.read_bytes()
text = source.decode()


def table(name):
    path = RESOLVED / name
    return path, list(csv.DictReader(path.open(), delimiter='\t'))


def function_body(name):
    return text.split('function ' + name + '(', 1)[1].split('function ', 1)[0]


def rawcode_int(code):
    return int.from_bytes(code.encode(), 'big')


def per_10k(value):
    scaled = Decimal(str(value)) * 10_000
    assert scaled == int(scaled), value
    return int(scaled)


def integer(value):
    assert Decimal(str(value)) == int(Decimal(str(value))), value
    return int(Decimal(str(value)))


shop_path, shop_rows = table('castle-shop-items.tsv')
items_path, item_rows = table('items.tsv')
mech_path, mech_rows = table('item-mechanics.tsv')
abilities_path, ability_rows = table('abilities.tsv')
fields_path, field_rows = table('object-fields.tsv')
protected_path, protected_rows = table('protected-ability-fields.tsv')
items = {r['rawcode']: r for r in item_rows}
mechanics = {r['item_rawcode']: r for r in mech_rows}
abilities = {}
for row in ability_rows:
    abilities.setdefault(row['rawcode'], {})[int(row['level'])] = row
fields = {}
for row in field_rows:
    fields[(row['category'], row['rawcode'], row['field_id'], int(row['level']))] = (
        row['recovered_value_json'] or row['last_write_value_json'])


def field(category, rawcode, field_id, level=0):
    return json.loads(fields[(category, rawcode, field_id, level)])


def item_art(rawcode):
    return field('items', rawcode, 'iico').replace('\\\\', '\\')


def labeled(ability, level=1):
    return json.loads(abilities[ability][level]['data_fields_labeled_json'])


def targets(ability, level=1):
    return abilities[ability][level]['targets']


# --- script assertions: shop order, buy flow, pickup flow, dummy caster proxies ---
slot_function = function_body('castleItemIdForSlot')
slot_ids = {int(m[1]): int(m[2]) for m in re.finditer(r'ODr==(\d+)\)then return (\d+)', slot_function)}
buy = function_body('buyCastleItem')
assert 'IssueNeutralImmediateOrderById(RDr,cX[__wurst_ensureInt(lGb[player_getId(RDr)])],SDr)' in buy
pickup = function_body('ZH')
assert 'xJq==rab)and' in pickup and 'VH' in pickup or '_I[' in pickup
constants = function_body('TH')
for name, value in [('tab', 1227894832), ('sab', 1227894836), ('rab', 1227894833),
                    ('qab', 1227894834), ('pab', 1750), ('oab', '29.'), ('nab', 4), ('mab', 5)]:
    assert f'{name}={value}' in constants, name
assert 'CreateUnitAtLoc(NJq,1697656888,QGb,.0)safeUnitAddAbility(OJq,1093678660)' in pickup
assert 'CreateUnitAtLoc(PJq,1697656888,QGb,.0)safeUnitAddAbility(QJq,1093678659)' in pickup
assert 'UnitApplyTimedLife(OJq,1112820806,oab)' in pickup
assert 'PLAYER_STATE_RESOURCE_FOOD_CAP' in function_body('YH')
count_staffs = function_body('VH')
assert 'LIq=(LIq+1)' in count_staffs
swap = function_body('WH')
assert 'UnitAddItemById(OIq,QIq)' in swap

spellbook = json.loads(abilities['A0A5'][1]['data_fields_json'])['DataA']
spell_list = spellbook.split(',')
assert len(spell_list) == 11
shop = []
position = 0
for entry in spell_list:
    if not entry.startswith('IBA'):
        continue
    slot = int(entry[3])
    item_raw = next(r for r in shop_rows if int(r['slot']) == slot)
    assert slot_ids[slot] == int(item_raw['item_rawcode_integer'])
    code = item_raw['item_rawcode']
    row = items[code]
    shop.append({
        'position': position,
        'slot': slot,
        'item': code,
        'button_ability': entry,
        'hotkey': field('abilities', entry, 'ahky'),
        'button_icon': field('abilities', entry, 'aart').replace('\\\\', '\\'),
        'runtime_button_cooldown_seconds': float(next(
            r for r in protected_rows if r['rawcode'] == entry and r['field'] == 'cooldown'
        )['runtime_value']),
        'stock': {
            'maximum': integer(row['stock_max']),
            'initial': integer(row['stock_initial']),
            'start_delay_seconds': integer(row['stock_start']),
            'replenish_interval_seconds': integer(row['stock_regen']),
        },
    })
    position += 1
assert [e['item'] for e in shop] == [
    'I009', 'I00A', 'I005', 'I001', 'I003', 'I004', 'I006', 'I00F', 'I008', 'I007']

filler_position = spell_list.index('AIsr')
# The spellbook ability has an authored column but no authored row: the missing row is the
# engine default of zero.
assert ('abilities', 'A0A5', 'abpy', 0) not in fields
spellbook_position = {'x': integer(field('abilities', 'A0A5', 'abpx')), 'y': 0}
spellbook_columns = 4


def cooldown_runtime(ability):
    rows = [r for r in protected_rows if r['rawcode'] == ability and r['field'] == 'cooldown']
    return float(rows[0]['runtime_value']) if rows else float(abilities[ability][1]['cooldown'])


def stat_label(row):
    return {k: row[k] for k in ['gold_cost', 'lumber_cost']}


def effects(code):
    row = items[code]
    ability_list = [a for a in row.get('abilities', '').split(',') if a]
    if code in ('I001', 'I002'):
        ability = 'A02D'
        assert ability_list == [ability] * (1 if code == 'I001' else 4)
        data = labeled(ability)
        return [{
            'kind': 'blast',
            'ability': ability,
            'copies': len(ability_list),
            'damage': integer(data['Initial Damage']),
            'radius': integer(abilities[ability][1]['area']),
            'cooldown_seconds': float(abilities[ability][1]['cooldown']),
            'missile_speed': integer(field('abilities', ability, 'amsp')),
            'missile_art': field('abilities', ability, 'amat').replace('\\\\', '\\'),
            'targets': targets(ability),
        }]
    if code == 'I003':
        data = labeled('A02F')
        return [{
            'kind': 'damage-aura', 'ability': 'A02F', 'buff': abilities['A02F'][1]['buffs'],
            'radius': integer(abilities['A02F'][1]['area']),
            'damage_per_10k': per_10k(data['Attack Damage Increase']),
            'targets': targets('A02F'),
        }]
    if code == 'I00A':
        data = labeled('A03T')
        return [{
            'kind': 'speed-aura', 'ability': 'A03T', 'buff': abilities['A03T'][1]['buffs'],
            'radius': integer(abilities['A03T'][1]['area']),
            'movement_percent': integer(Decimal(str(data['Movement Speed Increase (%)'])) * 100),
            'attack_speed_percent': integer(Decimal(str(data['Attack Speed Increase (%)'])) * 100),
            'targets': targets('A03T'),
        }]
    if code == 'I009':
        data = labeled('A008')
        return [{
            'kind': 'regeneration-aura', 'ability': 'A008',
            'radius': integer(abilities['A008'][1]['area']),
            'regeneration_per_second_per_10k': per_10k(data['Amount of Hit Points Regenerated']),
            'targets': targets('A008'),
        }]
    if code in ('I008', 'I007'):
        params = json.loads(mechanics[code]['parameters_json'])
        aura = params['aura_ability_rawcode']
        data = labeled(aura)
        assert params['carrier_unit_rawcode'] == 'e008'
        return [{
            'kind': 'team-damage-carrier', 'aura_ability': aura,
            'buff': abilities[aura][1]['buffs'],
            'radius': integer(abilities[aura][1]['area']),
            'damage_per_10k': per_10k(data['Attack Damage Increase']),
            'carrier_unit': params['carrier_unit_rawcode'],
            'carrier_lifetime_seconds': float(params['lifetime_seconds']),
            'targets': targets(aura),
        }]
    if code == 'I004':
        params = json.loads(mechanics[code]['parameters_json'])
        return [{
            'kind': 'legendary-point',
            'legendary_points': params['food_cap_delta_when_active'],
            'refund_gold_when_inactive': params['refund_gold_when_inactive'],
        }]
    if code in ('I005', 'I00F'):
        params = json.loads(mechanics[code]['parameters_json'])
        trigger = mechanics[code]['trigger_ability_rawcodes']
        effect = mechanics[code]['effect_ability_rawcodes']
        assert params['carrier_unit_rawcode'] == 'e008'
        assert [a for a in ability_list] == [trigger]
        common = {
            'trigger_ability': trigger, 'effect_ability': effect,
            'range': integer(abilities[trigger][1]['range']),
            'radius': integer(abilities[effect][1]['area']),
            'duration_seconds': float(field('abilities', effect, 'adur', 1)),
            'buff': abilities[effect][1]['buffs'],
            'targets': targets(effect),
            'carrier_unit': params['carrier_unit_rawcode'],
            'carrier_lifetime_seconds': float(params['dummy_lifetime_seconds']),
        }
        if code == 'I005':
            data = labeled(effect)
            common.update({
                'kind': 'point-scroll-restoration',
                'armor_bonus': integer(data['Defense Bonus']),
                'life': integer(data['Hit Points Gained']),
                'mana': integer(data['Mana Points Gained']),
            })
        else:
            data = labeled(effect)
            common.update({
                'kind': 'point-scroll-speed',
                'movement_speed_increase_per_10k': per_10k(data['Movement Speed Increase']),
            })
        return [common]
    if code == 'I006':
        params = json.loads(mechanics[code]['parameters_json'])
        effect = mechanics[code]['effect_ability_rawcodes']
        trigger = mechanics[code]['trigger_ability_rawcodes']
        levels = []
        for level in sorted(abilities[effect]):
            data = labeled(effect, level)
            levels.append({
                'level': level,
                'targets': integer(data['Number of Targets Hit']),
                'damage': integer(data['Damage per Target']),
                'damage_reduction_per_target_per_10k': per_10k(data['Damage Reduction per Target']),
                'jump_radius': integer(abilities[effect][level]['area']),
            })
        return [{
            'kind': 'orb-chain-lightning', 'trigger_ability': trigger, 'effect_ability': effect,
            'range': integer(abilities[trigger][1]['range']),
            'use_cooldown_seconds': cooldown_runtime(trigger),
            'level_step_minutes': params['effect_level_step_minutes'],
            'dummy_recycle_delay_seconds': float(params['dummy_recycle_delay_seconds']),
            'missile_speed': integer(field('abilities', trigger, 'amsp')),
            'targets': targets(trigger),
            'effect_levels': levels,
        }]
    raise AssertionError(code)


item_codes = [e['item'] for e in shop] + ['I002']
catalog = []
for code in item_codes:
    row = items[code]
    powerup = row['powerup'] == '1'
    entry = {
        'rawcode': code,
        'rawcode_integer': rawcode_int(code),
        'name': row['name'],
        'icon': item_art(code),
        'tooltip_title': field('items', code, 'utip'),
        'tooltip': field('items', code, 'utub'),
        'gold_cost': integer(row['gold_cost']),
        'lumber_cost': integer(row['lumber_cost']),
        'class': row['class'].lower(),
        'initial_charges': 0 if row['uses'] == '-' else integer(row['uses']),
        'stack_maximum': integer(row['stack_max']),
        'cooldown_group': row['cooldown_group'],
        'usable': row['usable'] == '1',
        'perishable': row['perishable'] == '1',
        'powerup': powerup,
        'droppable': row['droppable'] == '1',
        'drop_on_death': row['drop_on_death'] == '1',
        'pawnable': row['pawnable'] == '1',
        'sellable': row['sellable'] == '1',
        'effects': effects(code),
    }
    catalog.append(entry)

upgrade = json.loads(mechanics['I001']['parameters_json'])
assert upgrade == {'add_item_rawcode': 'I002', 'inventory_slot_max_index': 5,
                   'remove_item_rawcode': 'I001', 'required_count': 4}
slot_maximum = upgrade['inventory_slot_max_index'] + 1

artifact = {
    'map_version': '9.27',
    'retained_revision': 'r1',
    'source': {
        'script_sha256': hashlib.sha256(source).hexdigest(),
        'castle_shop_items_sha256': hashlib.sha256(shop_path.read_bytes()).hexdigest(),
        'items_sha256': hashlib.sha256(items_path.read_bytes()).hexdigest(),
        'item_mechanics_sha256': hashlib.sha256(mech_path.read_bytes()).hexdigest(),
        'abilities_sha256': hashlib.sha256(abilities_path.read_bytes()).hexdigest(),
        'object_fields_sha256': hashlib.sha256(fields_path.read_bytes()).hexdigest(),
        'protected_ability_fields_sha256': hashlib.sha256(protected_path.read_bytes()).hexdigest(),
        'script_functions': ['castleItemIdForSlot', 'buyCastleItem', 'TH', 'UH', 'VH', 'WH',
                             'YH', 'ZH', 'gL', 'iJ', 'xD'],
        'spellbook_ability': 'A0A5',
    },
    'inventory': {
        'slots': slot_maximum,
        'upgrades': [{
            'trigger_item': upgrade['remove_item_rawcode'],
            'required_count': upgrade['required_count'],
            'replacement_item': upgrade['add_item_rawcode'],
        }],
    },
    'shop': {
        'spellbook_ability': 'A0A5',
        'spellbook_hotkey': field('abilities', 'A0A5', 'ahky'),
        'spellbook_columns': spellbook_columns,
        'spellbook_position': spellbook_position,
        'filler_position': filler_position,
        'entries': shop,
    },
    'items': catalog,
}
output = ROOT / 'crates/sim/data/castle-fight/9.27/items-r1.json'
encoded = json.dumps(artifact, indent=2, sort_keys=True) + '\n'
if args.check:
    assert output.read_text() == encoded, 'items projection is stale'
else:
    output.write_text(encoded)
