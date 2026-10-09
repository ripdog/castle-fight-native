"""Reachable permanent mana improvement; production-spawn branches are excluded."""
import json
import re


def project(recipe, fields, mechanics, scaled):
    if recipe['kind'] == 'building-mana-regeneration':
        if mechanics['native_base'] != 'ANre' or set(fields['targs1'].split(',')) != {'structure','self'} or scaled(fields['datab1']) != 0:
            raise ValueError('building improvement requires flat self-only native mana regeneration')
        return dict(recipe, levels=[scaled(mechanics['improvement_levels']['dataa1'],10_000),scaled(mechanics['improvement_levels']['dataa2'],10_000)])
    semantic = mechanics['semantics_row']
    p = json.loads(semantic['parameters_json'])
    handler, cleanup, death = (mechanics[k] for k in ['improvement_handler','improvement_cleanup','improvement_death'])
    ability = int.from_bytes(recipe['source_key'].encode(),'big')
    child = int.from_bytes(p['improvement_ability_rawcode'].encode(),'big')
    marker = int.from_bytes(p['elemental_marker_rawcode'].encode(),'big')
    if (semantic['semantic_kind'] != 'permanent-special-building-mana-regen-improvement'
        or not all(p[k] for k in ['one_generator_per_target','target_requires_mana','target_rejects_production_buildings'])
        or p['unreachable_spawn_code_audit']['gameplay_reachable']
        or scaled(semantic['effective_mana_cost']) != 0 or scaled(semantic['effective_cooldown']) != 0
        or f'unit_removeAbility(apr,{ability})' not in handler
        or f'unit_getAbilityLevel(bpr,{child})>0' not in handler
        or '__wurst_safe_GetUnitState(bpr,UNIT_STATE_MAX_MANA)<1.' not in handler
        or '__wurst_ensureInt(hIb[buildingTypeIndexOfUnit(bpr)])>0' not in handler
        or f'UnitMakeAbilityPermanent(bpr,true,{child})' not in handler
        or f'unit_getAbilityLevel(bpr,{marker})>0' not in handler
        or f'SetUnitAbilityLevel(jpr,{child},{p["elemental_level"]})' not in handler
        or 'T0[cpr]=apr' not in handler or 'AddLightning("DRAM",false' not in handler
        or f'addProtectedAbility(Zor,{ability})' not in cleanup
        or 'T0[Wor]=nil' not in cleanup or 'DestroyLightning(Yor)' not in cleanup
        or f'unit_getAbilityLevel(M2q,{child})>0' not in death):
        raise ValueError('mana improvement requires exact permanent grant, target rejection and target-death cleanup')
    # The retained death path invokes cleanup through a protected global alias.
    import lua_index
    registration = re.search(r'_I\[_T\((\d+),"([0-9a-f]+)"\)\]=yK',mechanics["improvement_cleanup_registration"])
    guard = re.search(r'unit_getAbilityLevel\(M2q,'+str(child)+r'\)>0\)then _I\[\(_d\[\d+\]or _y\(\d+,_T\((\d+),"([0-9a-f]+)"\)\)\)\]\(M2q\)',death)
    if not registration or not guard or lua_index._decode_w3p_keyed_hex_string(int(registration[1]),registration[2].encode(),11351,1106) != lua_index._decode_w3p_keyed_hex_string(int(guard[1]),guard[2].encode(),11351,1106):
        raise ValueError('improvement death cleanup alias must resolve to the retained cleanup handler')
    child_fields = mechanics['improvement_levels']
    levels = [scaled(child_fields[f'dataa{level}'],10_000) for level in (p['normal_level'],p['elemental_level'])]
    if levels != [scaled(p['normal_mana_regen_bonus'],10_000),scaled(p['elemental_mana_regen_bonus'],10_000)] or set(fields['targs1'].split(',')) != {'friend','structure'}:
        raise ValueError('mana improvement native levels or friendly structure mask disagree')
    return dict(recipe, effect_ability_rawcode=p['improvement_ability_rawcode'], profile={
        'source_rawcode':int.from_bytes(recipe['unit_rawcode'].encode(),'big'), 'ability':ability, 'improvement':child,
        'elemental_marker':marker, 'range':scaled(fields['rng1'],1024), 'regeneration_per_second_per_10k':levels,
        'button':{'x':mechanics['improvement_ui']['abpx'],'y':mechanics['improvement_ui']['abpy']}, 'hotkey':mechanics['improvement_ui']['ahky']},
        provenance={'git_tree':mechanics['git_tree'],'semantic':'resolved/unit-spell-semantics.tsv',
            'handler':'improveSpecialBuilding,yK,fJ','protected_cleanup_alias':'cleanupElementalImprovement'})
