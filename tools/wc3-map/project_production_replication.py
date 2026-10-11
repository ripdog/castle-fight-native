"""Retain the training-event copy rules separately from a unit's native passives."""
import json
import re


def project(row, traces):
    p = json.loads(row['parameters_json'])
    source = lambda name: traces[name]['source']
    marker = p['type_murloc_marker_ability_id']
    local = source('EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning')
    shrine = source('spawnSyncedCompanions')
    trained = source('onUnitTrained')
    initialization = source('kP')
    exclusion_match = re.search(r'unit_getAbilityLevel\(cgs,(\d+)\)<=0\)and unit_isType', trained)
    marker_gate = f'unit_getAbilityLevel(eYl,{marker})>0'
    local_block = local.split('elseif')[0]
    twins_marker = p['twins_marker_ability_id']
    twins = p['twins_replacement_unit_ids']
    twins_block = local.split('elseif')[1] if local.count('elseif') == 1 else ''
    if (marker_gate not in local_block
        or local_block.count('__wurst_safe_CreateUnit(') != p['type_murloc_unconditional_local_extra_copy']
        or 'CreateUnit(gYl,unit_getTypeId(eYl),unit_getX(eYl),unit_getY(eYl),0.)' not in local_block
        or 'setupUnit(hYl,fYl)' not in local_block
        or 'Yfs=min(group_size(vGb[Xfs]),fR)' not in shrine
        or 'bgs=group_get(vGb[Xfs],Zfs)' not in shrine
        or 'GetRandomInt(0,99)<eR' not in shrine
        or shrine.count('setupUnit(__wurst_safe_CreateUnit(unit_getOwner(bgs),unit_getTypeId(Vfs),unit_getX(Vfs),unit_getY(Vfs),0.),Wfs)') != 1 + p['type_murloc_extra_clone_per_successful_shrine_roll']
        or f'unit_getAbilityLevel(Vfs,{marker})>0' not in shrine
        or int(re.search(r'\bfR=(\d+)', initialization)[1]) != p['maximum_shrines_checked']
        or int(re.search(r'\beR=(\d+)', initialization)[1]) != p['per_shrine_success_threshold_exclusive']
        or p['per_shrine_roll_min'] != 0 or p['per_shrine_roll_max'] != 99
        or exclusion_match is None
        or 'and unit_isType(dgs,UNIT_TYPE_STRUCTURE))then spawnSyncedCompanions(cgs,dgs)' not in trained):
        raise ValueError('training copies, ownership, setup, shrine rolls or eligibility changed')
    # The original trained carrier is retained. These are direct CreateUnit/setupUnit
    # calls, not native training events; shrine clones therefore do not spawn a pair.
    if (not p['twins_replacements_run_setup_unit'] or len(twins) != 2
        or f'unit_getAbilityLevel(eYl,{twins_marker})>0' not in twins_block
        or twins_block.count('__wurst_safe_CreateUnit(') != len(twins)
        or any(f'CreateUnit(gYl,{body},unit_getX(eYl),unit_getY(eYl),0.)' not in twins_block
               for body in twins)
        or 'setupUnit(iYl,fYl)' not in twins_block or 'setupUnit(jYl,fYl)' not in twins_block
        or 'RemoveUnit(' in local or 'KillUnit(' in local
        or str(twins_marker) in shrine or any(str(body) in shrine for body in twins)
        or 'onUnitTrained(' in local or 'onUnitTrained(' in shrine):
        raise ValueError('training companions, carrier lifetime or nonrecursive shrine setup changed')
    exclusion = int(exclusion_match[1])
    return {'shrine_rawcode': p['heroic_shrine_unit_id'], 'legendary_exclusion': exclusion,
            'marker': marker, 'local_extra_copies': p['type_murloc_unconditional_local_extra_copy'],
            'extra_copies_per_shrine': p['type_murloc_extra_clone_per_successful_shrine_roll'],
            'twins_marker': twins_marker, 'twins_children': twins,
            'maximum_shrines': p['maximum_shrines_checked'],
            'chance_per_100': p['per_shrine_success_threshold_exclusive']}
