"""Reconcile marker-driven damage callbacks with independent protected native dummy spells."""
from decimal import Decimal, ROUND_CEILING
import re


def project(recipe, mechanics, scaled):
    trace = mechanics['retaliation']
    handler = mechanics['retaliation_handler']
    dispatch = mechanics['retaliation_dispatch']
    match = re.search(r'GetRandomInt\((\d+),(\d+)\)<=\((\d+)\*ppq\)', handler)
    if (match is None or list(map(int, match.groups())) != [trace['random_roll_min'], trace['random_roll_max'], 15]
        or 'unit_isType(npq,UNIT_TYPE_MELEE_ATTACKER)' not in handler
        or 'not(BlzGetEventAttackType()==ATTACK_TYPE_NORMAL)' not in handler
        or f'unit_getAbilityLevel(gqq,{trace["target_marker_ability_id"]})>0' not in dispatch
        or '(widget_getLife(gqq)<=.405)' not in dispatch
        or 'jY=true __wurst_safe_IssueTargetOrderById(qpq,852095,npq)' not in handler
        or '__wurst_safe_SetUnitAbilityLevel(spq,1093682229,tpq)' not in handler
        or trace['proc_comparison'] != 'roll <= 15 * unit_level'):
        raise ValueError('retaliation requires its exact native melee/non-spell/inclusive-roll damage dispatcher')
    child = trace['thunderbolt_ability_id'].to_bytes(4, 'big').decode()
    fields = mechanics['retaliation_fields']
    protected = mechanics['retaliation_protected']
    if mechanics['retaliation_base'] != 'AHtb' or set(fields['targs1'].split(',')) != {'air','ground','debris','enemy','neutral','organic'}:
        raise ValueError('unsupported native retaliation child mask/base')
    profiles = []
    for level in range(1, scaled(fields['levels']) + 1):
        if scaled(protected[str(level)]['mana_cost']) != 0 or scaled('0' if fields.get(f'cast{level}', '-') in {'-', '_', ''} else fields[f'cast{level}']) != 0:
            raise ValueError('retaliation dummy must have its free instant native child')
        tick = lambda value: int((Decimal(value) * 30).to_integral_value(rounding=ROUND_CEILING))
        profiles.append({'ability': trace['thunderbolt_ability_id'], 'damage': scaled(fields[f'dataa{level}']),
            'stun_ticks': tick(fields[f'dur{level}']), 'hero_stun_ticks': tick(fields[f'herodur{level}']),
            'damage_per_second': 0, 'duration_ticks': 0,
            'speed_per_tick': int(Decimal(fields['missilespeed']) * 1024 / 30), 'cleanse': False, 'targets': 3})
    if len(profiles) != 2: raise ValueError('retaliation child level inventory changed')
    provenance = {'git_tree': mechanics['git_tree'], 'source':'resolved/production-unit-special-mechanics.tsv',
        'fields':'resolved/object-fields.tsv', 'protected':'resolved/protected-ability-fields.tsv',
        'handler':'handleTargetDamageEffects,castLightningShield', 'phase':'pre-health-commit',
        'native_melee_classification':'resolved/units.tsv:attack1_weapon_type=normal'}
    if recipe['kind'] == 'retaliation-projectile':
        return dict(recipe, profiles=profiles, provenance=provenance)
    level = trace['thunderbolt_ability_level']
    chance = scaled(trace['successful_roll_count'], 100)
    if scaled(trace['object_unit_level']) != level or scaled(trace['effective_proc_probability_percent'], 100) != chance:
        raise ValueError('retaliation unit level and inclusive roll disagree')
    return dict(recipe, effect_ability_rawcode=child, profile={'marker': trace['target_marker_ability_id'],
        'chance_per_10k': chance, 'range':scaled(fields.get(f'rng{level}', fields['rng1']),1024), 'organic_only':True,
        'projectile':profiles[level-1]}, provenance=provenance)
