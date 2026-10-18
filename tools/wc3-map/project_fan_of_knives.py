"""Project the damage-listener-issued native Fan of Knives (AEfk) into a scripted automatic profile."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, unit, protected, detail):
    trace = detail['fan_of_knives']
    functions = detail['functions']
    handler = functions['handleFanOfKnives']
    dispatch = functions['code__addAction_DamageRuntime']
    raw = int.from_bytes(recipe['source_key'].encode(), 'big')
    unit_id = int.from_bytes(recipe['unit_rawcode'].encode(), 'big')
    ticks = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding=ROUND_CEILING))
    scaled = lambda value, scale=1: int(Decimal(str(value)) * scale)
    cooldown = protected.get('cooldown', fields['cool1'])
    cost = protected.get('mana_cost', fields['cost1'])
    mask = set(fields['targs1'].split(','))
    object_data = trace['ability_object_data']
    if (detail['native_base'] != 'AEfk' or mask != {'air', 'ground', 'enemy', 'organic'}
        or trace['ability_id'] != raw or trace['issued_order'] != 'fanofknives'
        or trace['required_event_damage_type'] != 'normal' or trace['targets'] != fields['targs1']
        or not (trace['requires_ability_level_positive'] and trace['requires_cooldown_ready']
                and trace['requires_current_mana_at_least_native_cost'] and trace['resume_attack_immediately'])
        or Decimal(str(trace['area'])) != Decimal(fields['area1'])
        or Decimal(str(trace['effective_cooldown_seconds'])) != Decimal(cooldown)
        or Decimal(str(trace['effective_mana_cost'])) != Decimal(cost)
        or object_data['Damage Per Target'] != scaled(fields['dataa1'])
        or object_data['Maximum Total Damage'] != scaled(fields['datab1'])
        or object_data['Maximum Number of Targets'] != scaled(fields['datac1'])):
        raise ValueError('Fan of Knives requires its audited native ability and damage-listener trace')
    if (f'unit_getTypeId(iqq)=={unit_id}' not in handler
        or 'BlzGetEventDamageType()==DAMAGE_TYPE_NORMAL' not in handler
        or f'unit_getAbilityLevel(iqq,{raw})' not in handler
        or f'BlzGetUnitAbilityCooldownRemaining(iqq,{raw})<=0.' not in handler
        or f'BlzGetUnitAbilityManaCost(iqq,{raw}' not in handler
        or 'IssueImmediateOrder(iqq,"fanofknives")orderCodeAttack(iqq)' not in handler
        or 'handleFanOfKnives(tqq)' not in dispatch
        or 'GetTriggerUnit()' not in dispatch):
        raise ValueError('Fan of Knives control flow changed: damaged-unit listener, ready mana/cooldown gate and immediate order')
    rate = scaled(unit['mana_regen'], 10_000)
    return {
        'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {
            'mana': {'maximum': scaled(unit['mana_max']), 'starting': scaled(unit['mana_start']),
                     'regen_per_tick_per_10k': (1 << 31) | rate},
            'ability': {'id': raw, 'mana_cost': scaled(cost), 'cooldown_ticks': ticks(cooldown), 'range': 0,
                        'target_policy': 'AttackDamageTrigger',
                        'effect': {'FanOfKnives': {
                            'ability': raw, 'damage': scaled(fields['dataa1']),
                            'maximum_total_damage': scaled(fields['datab1']),
                            'maximum_targets': scaled(fields['datac1']),
                            'radius': scaled(fields['area1'], 1024), 'targets': 3, 'organic_only': True}}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None,
        'provenance': {'git_tree': detail['git_tree'], 'native': 'resolved/object-fields.tsv',
                       'protected': 'resolved/protected-ability-fields.tsv',
                       'script': 'resolved/production-unit-special-mechanics.tsv',
                       'handler': 'handleFanOfKnives,code__addAction_DamageRuntime'}}
