"""Native Heal order, actual effect mask, and an independent script resume callback."""
import json
import re
from decimal import Decimal, ROUND_CEILING

HEAL_MASK = {'air','friend','ground','invulnerable','neutral','nonancient','organic','self','vulnerable'}
SPRAY_MASK = {'air', 'friend', 'ground'}
SPRAY_ORDER = 'healingspray'
FULL_NAMES = {'vuln': 'vulnerable', 'invu': 'invulnerable'}


def tokens_of(value):
    return {FULL_NAMES.get(token, token) for token in value.split(',')}


def _spray(recipe, detail, handler, delay, tick):
    """Panda Monk Heal: the script sprays the healed unit's position from a free dummy at the caster."""
    semantics = detail['semantics_row']
    child = detail['effect_key']
    fields = detail['effect_fields']
    protected = detail['effect_protected']
    p = json.loads(semantics['parameters_json'])
    sites = p['delivery_sites']
    if (detail['all_bases'][child] != 'ANhs' or semantics['semantic_kind'] != 'proxy-object-effect'
        or len(sites) != 1 or sites[0]['callee'] != 'dummyCastPointFrom' or sites[0]['effect_rawcodes'] != [child]
        or tokens_of(fields['targs1']) != SPRAY_MASK
        or Decimal(protected.get('mana_cost', fields['cost1'])) != 0 or Decimal(fields['cool1']) != 0):
        raise ValueError('Healing Spray requires a free native friendly dummy channel')
    shape = (r'function \w+\((\w+),(\w+),(\w+)\)(?:local \w+=nil )+(\w+)=\2 (\w+)=\3 (\w+)=\4 '
        r'dummyCastPointFrom\(unit_getOwner\(\6\),(\d+),OrderId\("' + SPRAY_ORDER + r'"\),'
        r'unit_getPos\(\5\),unit_getPos\(\6\),([0-9.]+)\)(\w+)=\w+:create\d+\(\)\9\.u=\6 doAfter\(([0-9.]+),\9\)end')
    matched = re.fullmatch(shape, handler.strip())
    if matched is None or int(matched[7]) != int.from_bytes(child.encode(), 'big') or Decimal(matched[10]) != Decimal(delay):
        raise ValueError('Healing Spray must be cast from the caster at the healed unit with the retained resume delay')
    tokens = tokens_of(fields['targs1'])
    return {'ability': int.from_bytes(child.encode(), 'big'), 'radius': int(Decimal(fields['area1']) * 1024),
        'amount': int(Decimal(fields['dataa1'])), 'maximum_total': int(Decimal(fields['datad1'])),
        'waves': int(Decimal(fields['dataf1'])), 'interval_ticks': tick(fields['datab1']),
        # Deterministic interpretation: the first wave lands one wave interval after the cast.
        'first_wave_ticks': tick(fields['datab1']), 'lifetime_ticks': tick(matched[8]),
        'targets': (1 if 'ground' in tokens else 0) | (2 if 'air' in tokens else 0)}


def project(recipe, fields, unit, protected, detail):
    row, semantics = detail['mechanics_row'], detail['semantics_row']
    p = json.loads(semantics['parameters_json'])
    scripted_spray = semantics['semantic_kind'] == 'proxy-object-effect'
    if (detail['native_base'] != 'Ahea' or row['base_order'] != 'heal'
        or tokens_of(fields['targs1']) != HEAL_MASK):
        raise ValueError('Heal requires distinct native friendly effect and script trigger')
    if scripted_spray:
        if row['target_mode_label'] != 'ally-any' or row['mechanic_kind'] != 'dummy-point-ability':
            raise ValueError('Heal spray requires the any-ally dummy point dispatch')
    elif (row['target_mode_label'] != 'ally-ground' or row['mechanic_kind'] != 'scheduled-callback'
        or semantics['semantic_kind'] != 'registered-object-effect-plus-resume'
        or p['primary_effect_ability_rawcode'] != recipe['source_key']):
        raise ValueError('Heal requires distinct native friendly effect and ground script trigger')
    delays = json.loads(row['scheduled_delays_json'])
    if len(delays) != 1 or (not scripted_spray and Decimal(delays[0]) != Decimal(str(p['resume_attack_delay_seconds']))):
        raise ValueError('Heal resume deadline differs from retained callback')
    functions = detail['functions']
    handler = functions[row['handler_function']]
    tick = lambda value: int((Decimal(str(value))*30).to_integral_value(rounding=ROUND_CEILING))
    spray = None
    if scripted_spray:
        callback = functions[row['delayed_callback_functions'].split(',')[0]]
        if not re.fullmatch(r'function \w+\((\w+)\)if\(\(not\(\1\.u==nil\)\)and\(widget_getLife\(\1\.u\)>0\.405\)\)then orderCodeAttack\(\1\.u\)end end', callback.strip()):
            raise ValueError('Heal spray must resume the retained caster identity')
        spray = _spray(recipe, detail, handler, delays[0], tick)
    else:
        callback = functions['CallbackSingle_doAfter_RaceNelfAbilities_call_doAfter_RaceNelfAbilities1']
        if ('Qwn.u=Pwn' not in handler or f'doAfter({delays[0]},Qwn)' not in handler
            or 'orderCodeAttack(jwn.u)' not in callback):
            raise ValueError('Heal must retain caster identity and delayed attack order')
    raw = int.from_bytes(recipe['source_key'].encode(), 'big')
    mana = lambda key, scale=1: 0 if unit[key] in {'','-','_'} else int(Decimal(unit[key])*scale)
    rate = mana('mana_regen', 10000)
    heal = {'amount':int(fields['dataa1']),'targets':3,'organic_only':True,'nonancient_only':True,
        'affects_invulnerable':True,'resume_attack_ticks':tick(delays[0])}
    if spray is not None:
        heal['spray'] = spray
    return {'kind':'scripted-automatic','source_kind':recipe['source_kind'],'source_key':recipe['source_key'],'unit_rawcode':recipe['unit_rawcode'],
        'spellcasting':{'mana':{'maximum':mana('mana_max'),'starting':mana('mana_start'),'regen_per_tick_per_10k':(1<<31)|rate},
        'ability':{'id':raw,'mana_cost':int(protected.get('mana_cost',fields['cost1'])), 'cooldown_ticks':tick(protected.get('cooldown',fields['cool1'])),
        'range':int(Decimal(fields['rng1'])*1024),'target_policy':'WoundedFriendlyUnit','effect':{'Heal':heal}}}, 'mana_regen_per_second_per_10k':rate,
        'provenance':{'git_tree':detail['git_tree'],'native':'resolved/object-fields.tsv','script':'resolved/unit-spell-mechanics.tsv','semantics':'resolved/unit-spell-semantics.tsv'}}
