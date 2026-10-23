"""Native Grab Tree (`Agra`): the conditional club weapon enabled at spawn for a limited number of attacks."""
import re
from decimal import Decimal, ROUND_CEILING, ROUND_HALF_UP

DAMAGE_TYPES = {'normal': 'Normal', 'pierce': 'Pierce', 'siege': 'Siege', 'magic': 'Magic', 'chaos': 'Chaos',
    'spells': 'Spells', 'hero': 'Hero'}


def project(recipe, fields, detail):
    ticks = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding=ROUND_CEILING))
    unit = recipe['unit_rawcode']
    source = recipe['source_key']
    rows = [row for row in detail['attack_rows'] if row['unit_rawcode'] == unit and row['activation'] != 'default'
        or row['unit_rawcode'] == unit and row['disabled_by_ability'] == source]
    club = [row for row in rows if row['attack_index'] == '2']
    primary = [row for row in rows if row['attack_index'] == '1']
    p = detail['war_club']
    entered = detail['functions']['onUnitEnteredMap']
    tree = detail['functions']['castNatureAttackTree']
    type_id = int.from_bytes(unit.encode(), 'big')
    if (detail['native_base'] != 'Agra' or len(club) != 1 or len(primary) != 1
            or club[0]['activation'] != 'conditional' or club[0]['enabled_by_ability'] != source
            or primary[0]['disabled_by_ability'] != source
            or club[0]['conditional_max_attacks'] != str(int(Decimal(fields['datae1'])))
            or int(Decimal(fields['datac1'])) != 0 or int(Decimal(fields['datad1'])) != 1
            or f'qcs=={type_id})then castNatureAttackTree(ocs)return' not in entered
            or f'IssueTargetOrderById(hcs,{p["grab_tree_order_id"]},lcs)' not in tree
            or not re.search(r'doAfter\(%s,mcs\)end$' % re.escape(str(p['resume_attack_delay_seconds'])), tree)
            or not p['bypasses_wrong_order_guard']
            or ticks(fields['dataa1']) > ticks(p['resume_attack_delay_seconds'])
            or int(p['war_club_ability_id']) != int.from_bytes(source.encode(), 'big')):
        raise ValueError('War Club control flow changed: conditional second weapon, grab order and resume delay')
    row = club[0]
    tokens = set(row['targets'].split(','))
    movement = tokens & {'air', 'ground', 'structure'}
    if tokens - {'air', 'ground', 'structure', 'enemies', 'enemy'} or not movement or row['weapon_type'] != 'normal':
        raise ValueError('War Club requires a melee weapon with an explicit unit/structure mask')
    damage = Decimal(row['avg_damage']).quantize(Decimal(1), rounding=ROUND_HALF_UP)
    return {**recipe, 'profile': {'ability': int.from_bytes(source.encode(), 'big'),
        'damage': int(damage), 'range': int(Decimal(row['range']) * 1024), 'cooldown_ticks': ticks(row['cooldown']),
        'targets': sum(bit for token, bit in [('ground', 1), ('air', 2), ('structure', 4)] if token in movement),
        'damage_type': DAMAGE_TYPES[row['attack_type']], 'max_attacks': int(row['conditional_max_attacks']),
        'attach_delay_ticks': ticks(fields['dataa1']), 'resume_delay_ticks': ticks(p['resume_attack_delay_seconds'])},
        'provenance': {'source': 'resolved/production-unit-attacks.tsv', 'native_base': 'Agra',
            'script': 'castNatureAttackTree', 'remove_delay': fields['datab1']}}
