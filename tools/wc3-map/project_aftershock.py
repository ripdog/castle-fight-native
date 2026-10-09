"""Project health-scaled nested damage from its retained dispatcher and registration."""
import re


def project(recipe, mechanics, scaled):
    trace = mechanics['aftershock']
    handler = mechanics['aftershock_handler']
    marker = trace['aftershock_marker_ability_id']
    dispatcher = trace['source_damage_marker_ability_id']
    formula = re.search(r'\(\(\((\d+)\.\*unit_getLevel\(Qpq\)\)\*widget_getLife\(Qpq\)\)/unit_getMaxHP\(Qpq\)\)', handler)
    if (formula is None or f'unit_getAbilityLevel(Qpq,{dispatcher})<=0' not in handler
        or f'elseif(unit_getAbilityLevel(Qpq,{marker})>0)' not in handler
        or 'dqq=hhb __wurst_safe_DisableTrigger(dqq)' not in handler
        or 'eqq=hhb __wurst_safe_EnableTrigger(eqq)' not in handler
        or 'true,false,ATTACK_TYPE_SIEGE,DAMAGE_TYPE_DEMOLITION,WEAPON_TYPE_WHOKNOWS' not in handler
        or 'handleSourceDamageEffects(sqq,tqq)handleTargetDamageEffects(sqq,tqq)' not in mechanics['damage_runtime']
        or 'if(not isCombatSapper(ocs))then return end' not in mechanics['sapper_registration']
        or 'registerSapperDamage(ocs)' not in mechanics['sapper_registration']
        or 'TriggerRegisterUnitEvent(hhb,vqq,EVENT_UNIT_DAMAGED)' not in mechanics['damage_registration']
        or trace['bonus_damage_formula'] != f'{formula[1]} * unit_level * current_hp / max_hp'
        or not trace['recursion_guard_trigger_disabled_during_bonus_hit']
        or (trace['attack_type'],trace['damage_type'],trace['is_attack'],trace['is_ranged']) != ('siege','demolition',True,False)):
        raise ValueError('aftershock requires its health formula, guarded nested damage and sapper-only source-before-target dispatcher')
    maximum = scaled(formula[1]) * scaled(trace['object_unit_level'])
    if maximum != trace['maximum_bonus_damage_at_full_hp'] or trace['unit_level'] != scaled(trace['object_unit_level']):
        raise ValueError('aftershock source object level disagrees with its retained trace')
    return dict(recipe, profile={'marker':marker,'dispatcher':dispatcher,'maximum_damage':maximum},
        provenance={'git_tree':mechanics['git_tree'], 'source':'resolved/production-unit-special-mechanics.tsv',
            'handler':'handleSourceDamageEffects', 'registration':'onUnitEnteredMap,registerSapperDamage',
            'phase':'source-before-target-pre-health-commit', 'nested_damage':'hhb-disabled-independent-listeners-active'})
