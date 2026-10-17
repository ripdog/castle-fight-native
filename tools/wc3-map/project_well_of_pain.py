"""Native building parent plus the literal Well of Pain callback, from retained source only."""
import json
import re
from decimal import Decimal, ROUND_CEILING
from project_naga_unit import project_parasite


def project(row, fields, units, abilities, protected, traces, version):
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    integer = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda value: integer(value, 30)
    params = json.loads(row['parameters_json'])
    source = lambda name: traces[name]['source']
    parent = fields[row['ability_rawcode']]
    spell = source('wellOfPainSpell')
    wounded = source('ForGroupCallback_forUnitsInRect_RaceCorruptedAbilities_callback_forUnitsInRect_RaceCorruptedAbilities')
    fallback = source('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers')
    damage = re.search(r'Ulr=ATTACK_TYPE_NORMAL Vlr=DAMAGE_TYPE_SONIC Wlr=WEAPON_TYPE_WHOKNOWS __wurst_safe_UnitDamageTarget\(Slr,Tlr,(\d+)\.,true,false,Ulr,Vlr,Wlr\)', spell)
    ghost = re.search(r'createUnit\(\w+,(\d+),unit_getPos', spell)
    if (row['mechanic_kind'] != 'damage-with-on-kill-summon'
            or 'Qlr=randomWoundedEnemy(Plr)' not in spell or 'Qlr=randomAliveEnemy(Plr)' not in spell
            or 'if(not checkForShield(Qlr))then' not in spell or not damage or not ghost
            or 'widget_getLife(Qlr)<0.405' not in spell
            or int(damage[1]) != params['damage'] or int(ghost[1]) != params['on_kill_unit_id']
            or params['damage_type'] != 'sonic'):
        raise ValueError('Well of Pain requires wounded-first selection, one shield attempt, sonic damage and kill summon')
    wounded_exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(Pln,(\d+)\)<=0', wounded)))
    fallback_exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(zOn,(\d+)\)<=0', fallback)))
    wounded_type = re.search(r'not\(unit_getTypeId\(Pln\)==(\d+)\)', wounded)
    fallback_type = re.search(r'not\(unit_getTypeId\(zOn\)==(\d+)\)', fallback)
    if (len(wounded_exclusions) != 3 or wounded_exclusions[:2] != fallback_exclusions or not wounded_type
            or not fallback_type or wounded_type[1] != fallback_type[1]
            or 'widget_getLife(Pln)<unit_getMaxHP(Pln)' not in wounded or 'isAliveCombatSapper(Pln)' not in wounded):
        raise ValueError('Well of Pain selectors must differ only by wounded state and one extra marker')
    art = re.search(r'addEffect1\("((?:[^"\\]|\\.)*\.mdl)",Qlr,"(\w+)"\)', spell)
    if not art:
        raise ValueError('Well of Pain target art changed')
    a = next(a for a in abilities if a['rawcode'] == row['ability_rawcode'] and a['level'] == '1')
    if (a['base_rawcode'] != 'ACpa'
            or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}
            or parent['abuf:1'] != '' or any(Decimal(str(parent[f'Poi{i}:1'])) != 0 for i in range(1, 5))):
        raise ValueError('building trigger requires its empty, zero-effect native Parasite parent')
    cost = lambda ability, field: next(p['runtime_value'] for p in protected if p['rawcode'] == ability and p['field'] == field)
    profile = {'map_version': version, 'trigger_targets': 5, 'trigger_invulnerable': True,
        'trigger_spell_immune': False, 'trigger_mechanical_only': False,
        'selector_exclusions': fallback_exclusions, 'selector_excluded_rawcode': int(fallback_type[1]),
        'wounded_exclusion': wounded_exclusions[2], 'damage': int(damage[1]), 'ghost_rawcode': int(ghost[1])}
    profile['parent'] = project_parasite({'buffid1': parent['native_base_buff'].split(',')[0],
        'missilehoming': parent['amho:0'], 'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'],
        'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'], 'unitid1': parent['ipmu:1'],
        'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    unit = units[row['building_rawcode']]
    amount = lambda k: 0 if unit[k].strip() in {'', '-', '_'} else integer(unit[k])
    return {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
        'regen_per_tick_per_10k': (1 << 31) | integer(unit['mana_regen'], 10000)},
        'ability': {'id': raw(row['ability_rawcode']), 'mana_cost': integer(cost(row['ability_rawcode'], 'mana_cost')),
        'cooldown_ticks': tick(cost(row['ability_rawcode'], 'cooldown')), 'range': integer(parent['aran:1'], 1024),
        'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'WellOfPain': profile}}}
