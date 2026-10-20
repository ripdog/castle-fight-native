"""Native Parasite and independent delayed AHtc/script mana burn, from source control flow."""
from decimal import Decimal, ROUND_CEILING
import re
import json

from project_naga_unit import project_parasite


def project(recipe, fields, unit, protected, detail):
    integer = lambda v, scale=1: int((Decimal(str(v)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda v: integer(v, 30)
    raw = lambda v: int.from_bytes(v.encode('ascii'), 'big')
    row = detail['mechanics_row']
    functions = detail['functions']
    handler = functions[row['handler_function']]
    callback = functions[row['delayed_callback_functions'].split(',')[0]]
    # The ForGroup picker is the script's independent recipient filter. Each race owns a copy.
    pickers = [name for name in row.get('dynamic_callback_functions', '').split(',') if name.startswith('ForGroupCallback_')]
    picker = functions[pickers[-1]] if pickers else functions[
        'ForGroupCallback_forUnitsInRange_doAfter_RaceChaosAbilities_callback_forUnitsInRange_doAfter_RaceChaosAbilities']
    number = r'(\d+(?:\.\d*)?)'
    release = re.search(
        r'(?P<point>\w+)=tupleCopy1\(unit_getPos\((?P<target>\w+)\)\)'
        r'(?:(?P<mover>\w+)=createUnit\((?P<owner>\w+),(?P<code>\d+),unit_getPos\((?P<caster>\w+)\),\{0\.\}\))'
        r'unit_issuePointOrderById\((?P=mover),851984,(?P=point)\)'
        r'(?P<alias>\w+)=(?P=mover) (?P<alias2>\w+)=(?P=alias) __wurst_safe_UnitApplyTimedLife\((?P=alias2),\d+,(?P<life>' + number[1:-1] + r')\)'
        r'(?P<delay>\w+)=\(vec2_distanceTo\(unit_getPos\((?P=caster)\),(?P=point)\)/(?P<speed>' + number[1:-1] + r')\)'
        r'(?P<state>\w+)=\w+:create\d+\(\)(?P=state)\.p=(?P=owner) (?P=state)\.tpos=tupleCopy1\((?P=point)\)doAfter\((?P=delay),(?P=state)\)', handler)
    if release is None:
        # Chaos script: release is split around the shared point snapshot.
        speed = re.search(r'Hln=\(vec2_distanceTo\(unit_getPos\(Bln\),Eln\)/' + number + r'\)', handler)
        mover = re.search(r'Fln=createUnit\(Dln,(\d+),unit_getPos\(Bln\),\{0\.\}\)', handler)
        lifetime = re.search(r'UnitApplyTimedLife\(Jln,\d+,' + number + r'\)', handler)
        if (not all([speed, mover, lifetime]) or 'Eln=tupleCopy1(unit_getPos(Cln))' not in handler
                or 'unit_issuePointOrderById(Fln,851984,Eln)' not in handler
                or 'Gln.p=Dln Gln.tpos=tupleCopy1(Eln)doAfter(Hln,Gln)' not in handler):
            raise ValueError('travelling clap release, point snapshot or delay changed')
        speed_value, mover_value, life_value = speed[1], mover[1], lifetime[1]
    else:
        speed_value, mover_value, life_value = release['speed'], release['code'], release['life']
    child = re.search(r'dummyCarrierCastImmediate\((\w+)\.p,(\d+),852096,\1\.tpos,' + number + r'\)', callback)
    radius = re.search(r'forUnitsInRange\(\w+,' + number + r',false,\w+\)', callback)
    maximum = re.search(r'(?P<mana>\w+)=unit_getMana\(\w+\)if\((?P=mana)>0\.\)then (?P<burn>\w+)=min1\((?P=mana),' + number + r'\)', callback)
    shield = re.search(r'unit_getAbilityLevel\(\w+,(\d+)\)<=0\)\)then group_add', picker)
    if (row['target_mode_label'] != 'enemy-ground-combat-sapper'
        or not all([child, radius, maximum, shield])
        or not re.search(r'\w+=\(' + maximum['mana'] + '-' + maximum['burn'] + r'\)', callback)
        or 'ATTACK_TYPE_CHAOS' not in callback or 'DAMAGE_TYPE_NORMAL' not in callback
        or not re.search(r'__wurst_safe_UnitDamageTarget\(\w+,\w+,\w+,true,false,\w+,\w+,\w+\)', callback)
        or not re.search(r'isAliveCombatSapper\((\w+)\)and unit_isEnemyOf\(\1,\w+\.p\)', picker)
        or not re.search(r'unit_isType\(\w+,UNIT_TYPE_GROUND\)', picker)
        or f'unit_getAbilityLevel' not in picker or f',{raw("Avul")})<=0' not in picker):
        raise ValueError('travelling clap release, point snapshot, delay or independent mana picker changed')
    speed = [None, speed_value]
    mover = [None, mover_value]
    lifetime = [None, life_value]
    radius = [None, radius[1]]
    maximum = [None, maximum[3]]
    child_code = int(child[2]).to_bytes(4, 'big').decode()
    native = detail['all_fields'][child_code]
    parent_code = row['ability_rawcode']
    parent = detail['all_fields'][parent_code]
    if (detail['all_bases'][parent_code] != 'ACpa'
        or detail['all_bases'][child_code] != 'AHtc'
        or set(native['targs1'].split(',')) != {'enemies', 'ground'}
        or set(parent['targs1'].split(',')) != {'enemies', 'ground', 'neutral', 'nonhero', 'vulnerable'}
        or any(Decimal(detail['all_protected'][child_code][field]) != 0 for field in ['mana_cost', 'cooldown'])
        or integer(native['datab1']) != 0):
        raise ValueError('travelling clap requires independent free native child and nonhero Parasite parent')
    mover_code = int(mover[1]).to_bytes(4, 'big').decode()
    body = detail['all_units'][mover_code]
    mover_fields = detail['all_unit_fields'][mover_code]
    if body['move_type'] != 'fly' or set(body['abilities'].split(',')) != {'Aloc', 'Avul'}:
        raise ValueError('travelling clap requires its unselectable flying dummy')
    child_only = recipe['source_kind'] == 'ability-effect'
    clap = {'ability': raw(child_code), 'radius': integer(native['area1'], 1024),
        'primary_damage': integer(native['datab1']), 'area_damage': integer(native['dataa1']),
        'duration_ticks': tick(native['dur1']), 'hero_duration_ticks': tick(native['herodur1']),
        'movement_percent_delta': -integer(native['datac1'], 100),
        'attack_speed_percent_delta': -integer(native['datad1'], 100)}
    p = {'parent': project_parasite(parent, detail['map_version']), 'parent_nonhero_only': True,
        'mover_rawcode': raw(mover_code), 'callback_speed_per_second': integer(speed[1], 1024),
        'mover_speed_per_tick': int(Decimal(detail['all_unit_stats'][mover_code]['unitstat_move_speed']) * 1024 / 30),
        'mover_has_visible_model': mover_fields['file'].lower() not in {'none.mdl', '_', '-', ''},
        'mover_height_world_units': integer(mover_fields['moveheight']),
        'mover_duration_ticks': tick(lifetime[1]), 'clap': clap,
        'burn_radius': integer(radius[1], 1024), 'maximum_mana_burn': integer(maximum[1]),
        'excluded_ability': int(shield[1]), 'feedback_visual_ability': raw(parent_code)}
    art = re.search(r'addEffect1\("(.*?)",\w+,"(.*?)"\)', callback)
    if art is None or art[2] != 'origin':
        raise ValueError('script mana burn requires its recipient origin effect')
    visuals = [] if child_only else [{'owner_rawcode': parent_code,
        'source_unit_rawcode': None, 'script_source_unit_rawcode': recipe['unit_rawcode'], 'role': 'mana-burn-target',
        'model_path': json.loads('"' + art[1] + '"')}]
    mana = lambda field, scale=1: 0 if unit[field].strip() in {'-', '_', ''} else integer(unit[field], scale)
    rate = mana('mana_regen', 10000)
    effect = {'FrostNova': {**clap, 'targets': 1}} if child_only else {'TravellingManaClap': p}
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'],
        'source_key': recipe['source_key'], 'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {'mana': {'maximum': mana('mana_max'), 'starting': mana('mana_start'),
            'regen_per_tick_per_10k': (1 << 31) | rate}, 'ability': {
            'id': raw(recipe['source_key']), 'mana_cost': 0 if child_only else integer(protected['mana_cost']),
            'cooldown_ticks': 0 if child_only else tick(protected['cooldown']),
            'range': 0 if child_only else integer(parent['rng1'], 1024),
            'target_policy': 'ScriptedController' if child_only else 'RandomGroundEnemyUnit', 'effect': effect}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None if child_only else child_code,
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/object-fields.tsv',
            'script': 'resolved/unit-spell-mechanics.tsv', 'script_visuals': visuals, 'functions': [row['handler_function'],
                row['delayed_callback_functions'],
                pickers[-1] if pickers else 'ForGroupCallback_forUnitsInRange_doAfter_RaceChaosAbilities_callback_forUnitsInRange_doAfter_RaceChaosAbilities']}}
