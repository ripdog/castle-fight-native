"""The Bears' target-damage listener: Feral Rage (dummy Inner Fire and Bloodlust) and hibernation."""
import re
from decimal import Decimal, ROUND_CEILING

BLOODLUST_BASE = 'ACbb'
INNER_FIRE_BASE = 'Ainf'
ITEM_REGENERATION_BASE = 'Arll'
ITEM_DAMAGE_BASE = 'AItx'


def project(recipe, detail):
    p = detail['feral_bear']
    functions, fields, bases = detail['functions'], detail['all_fields'], detail['all_bases']
    ticks = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big') if isinstance(value, str) else value
    code = lambda number: number.to_bytes(4, 'big').decode()
    unit = int.from_bytes(recipe['unit_rawcode'].encode(), 'big')
    marker = int.from_bytes(recipe['source_key'].encode(), 'big')
    listener, rage, begin, asleep, finish = (functions[name] for name in
        ('handleTargetDamageEffects', 'feralRage', 'startBearHibernate', 'startBearSleep', 'finishBearHibernate'))
    damage_ability, speed_ability = p['feral_rage_damage_ability_id'], p['feral_rage_attack_speed_ability_id']
    chance, level = p['feral_rage_proc_chance_percent'], p['hibernate_level']
    sleep, regeneration = p['hibernate_sleep_ability_id'], p['hibernate_regen_ability_id']
    enabled, threshold = p['hibernate_enabled_ability_id'], p['hibernate_trigger_life_below']
    extra = p['hibernate_extra_sleep_ability_id']
    branch = (rf'hqq=={unit}\)then feralRage\(gqq,{chance},{damage_ability},{speed_ability}\)if\(\(unit_getAbilityLevel\(gqq,{enabled}\)>0\)'
        rf'and\(widget_getLife\(gqq\)<{threshold}\.\)\)then startBearHibernate\(gqq,{level}\)else unit_removeAbility\(gqq,{sleep}\)'
        rf'unit_removeAbility\(gqq,{regeneration}\)')
    gate = f'unit_getAbilityLevel(gqq,{p["runtime_damage_marker_ability_id"]})<=0))then return end'
    rage_shape = (rf'if\(GetRandomInt\(0,99\)<Woq\)then.*addProtectedAbility\(epq,Xoq\)unit_issueTargetOrderById\(dpq,{p["feral_rage_damage_order_id"]},Voq\)'
        r'__wurst_safe_UnitApplyTimedLife\(dpq,1112820806,2\.\).*addProtectedAbility\(ipq,Yoq\)unit_issueTargetOrderById\(hpq,'
        rf'{p["feral_rage_attack_speed_order_id"]},Voq\)__wurst_safe_UnitApplyTimedLife\(hpq,1112820806,2\.\)end end')
    sleep_shape = (rf'__wurst_safe_SetUnitMoveSpeed\(Ipq,{p["hibernate_sleep_move_speed"]}\.\)__wurst_safe_UnitRemoveBuffs\(Fpq,true,true\)jY=true '
        rf'unit_issueImmediateOrderById\(Fpq,{p["hibernate_sleep_order_id"]}\)Jpq=Fpq addProtectedAbility\(Jpq,{sleep}\)')
    if (not re.search(branch, listener) or gate not in listener or not re.search(rage_shape, rage.replace('\n', ''))
            or not re.search(sleep_shape, asleep)
            or f'SetUnitMoveSpeed(Npq,{p["hibernate_retreat_move_speed"]}.)issueCodeTowardsOwnCastle(Npq)' not in begin
            or f'doAfter({p["hibernate_retreat_seconds"]}.,Ppq)' not in begin
            or f'doAfter({p["hibernate_sleep_seconds"]}.,Hpq)' not in asleep
            or 'orderCodeAttack(Dpq)' not in finish or f'unit_removeAbility(Dpq,{sleep})' not in finish
            or not p['hibernate_marker_removed_on_trigger'] or not p['resume_attack_after_hibernate']
            or not p['hibernate_remove_positive_and_negative_buffs_before_sleep']
            or p['hibernate_retreat_direction'] != 'toward-own-castle'):
        raise ValueError('Bear listener control flow changed: Feral Rage dummies, one-time retreat, sleep, regeneration and wake order')
    damage_fields, speed_fields = fields[code(damage_ability)], fields[code(speed_ability)]
    regen_fields = fields[code(regeneration)]
    if (bases[code(damage_ability)] != INNER_FIRE_BASE or bases[code(speed_ability)] != BLOODLUST_BASE
            or bases[code(regeneration)] != ITEM_REGENERATION_BASE
            or ticks(damage_fields['dur1']) != ticks(p['feral_rage_buff_duration_seconds'])
            or ticks(speed_fields['dur1']) != ticks(p['feral_rage_buff_duration_seconds'])
            or Decimal(speed_fields['datab1']) != 0 or Decimal(damage_fields['datab1']) != 0
            or Decimal(damage_fields['datad1']) != 0
            or Decimal(damage_fields['dataa1']) != Decimal(str(p['feral_rage_damage_object_data']['Damage Increase (%)']))
            or Decimal(speed_fields['dataa1']) != Decimal(str(p['feral_rage_attack_speed_object_data']['Attack Speed Increase (%)']))
            or Decimal(regen_fields['dataa1']) != Decimal(str(p['hibernate_regen_object_data']['Hit Points Regenerated Per Second']))):
        raise ValueError('Feral Rage dummies must be the audited free Inner Fire and Bloodlust rows')
    def buff(ability):
        row = fields[code(ability)]
        tokens = set(row['targs1'].split(','))
        return {'rawcode': raw(row['buffid1']), 'positive': True, 'stealable': row.get('hero', '0') == '0',
            'organic_only': 'organic' in tokens}
    bonus = None
    if extra is not None:
        extra_fields = fields[code(extra)]
        if (bases[code(extra)] != ITEM_DAMAGE_BASE
                or Decimal(extra_fields['dataa1']) != Decimal(str(p['hibernate_extra_sleep_object_data']['Attack Bonus']))):
            raise ValueError('wake bonus must be the audited item attack bonus')
        bonus = {'ability': extra, 'effect': {'AttackDamage': int(Decimal(extra_fields['dataa1']))}}
    return {**recipe, 'profile': {
        'ability': marker, 'rage_chance_per_10k': chance * 100,
        'rage_duration_ticks': ticks(p['feral_rage_buff_duration_seconds']),
        'rage_damage_ability': damage_ability, 'rage_damage_buff': buff(damage_ability),
        'rage_damage_bonus_per_10k': int(Decimal(damage_fields['dataa1']) * 10_000),
        'rage_speed_ability': speed_ability, 'rage_speed_buff': buff(speed_ability),
        'rage_attack_speed_percent_delta': int(Decimal(speed_fields['dataa1']) * 100),
        'hibernate_below_health': threshold,
        'retreat_ticks': ticks(p['hibernate_retreat_seconds']),
        'retreat_speed_per_tick': int(Decimal(p['hibernate_retreat_move_speed']) * 1024 // 30),
        'sleep_ticks': ticks(p['hibernate_sleep_seconds']),
        'sleep_regeneration_ability': regeneration,
        'sleep_regeneration_per_second_per_10k': int(Decimal(regen_fields['dataa1']) * 10_000),
        'wake_bonus': bonus},
        'provenance': {'source': 'resolved/production-unit-special-mechanics.tsv',
            'handler': 'handleTargetDamageEffects', 'sleep_ability': sleep, 'sleep_speed': p['hibernate_sleep_move_speed']}}
