"""Source-owned team support and owner-scaled native death healing."""
import json
import re
from decimal import Decimal
from lua_index import _decode_lua_short_string_contents, _decode_w3p_string_payload


def project(row, buckets, traces):
    p = json.loads(row['parameters_json'])
    source = lambda name: traces[name]['source']
    init, death, target = source('ME'), source('fJ'), source('wL')
    for symbol, key in [('Ftb', 'heal_per_owned_elemental_production_building'),
                        ('Etb', 'maximum_heal')]:
        match = re.search(r'\b' + symbol + r'=([\d.]+)', init)
        if not match or Decimal(match[1]) != Decimal(str(p[key])):
            raise ValueError('death-heal formula constants differ from native initialization')
    if ('return min((Ftb*eleBuildingTotal(bCp)),Etb)' not in source('eleLinkerHealAmount')
        or 'TX=eleLinkerHealAmount(P2q)' not in death
        or '(__wurst_ensureInt(Atb[R2q])>0)' not in death
        or f'unit_getAbilityLevel(M2q,{p["elemental_unit_marker_ability_id"]})>0' not in death
        or f'{p["heal_radius"]}.,C3q' not in death
        or 'unit_getTypeId(jHr)==UX' not in target
        or 'isAliveCombatSapper(jHr)and unit_isAllyOf(jHr,mIb)' not in target
        or Decimal(re.search(r'\(([.\d]+)\*TX\)', target)[1]) != Decimal(str(p['other_unit_type_heal_factor']))):
        raise ValueError('death-heal owner/team/type/target control flow changed')
    if ('if(not(N2q==nil))then' not in death or 'UNIT_TYPE_SUMMONED' not in death or 'unit_getAbilityLevel(M2q,1093678919)<=0' not in death
        or 'unit_getAbilityLevel(M2q,1093678920)<=0' not in death):
        raise ValueError('death-heal legendary/summoned exclusions changed')
    finish = source('onBuildingFinished')
    if ('if(WDp and(unit_getAbilityLevel(VDp,1093682259)>0))then acquireEleBuilding(VDp)' not in finish
        or f'YDp=={p["linker_unit_id"]})then acquireLinker' not in finish
        or 'onBuildingFinished(GetTriggerUnit(),true)' not in source('EventListener_add_BuildingLifecycle_onEvent_add_BuildingLifecycle')
        or 'onBuildingFinished(GetTriggerUnit(),false)' not in source('EventListener_add_BuildingLifecycle_onEvent_add_BuildingLifecycle1')):
        raise ValueError('death-heal construction/upgrade registration changed')
    if len(buckets) != p['resolved_elemental_building_count']:
        raise ValueError('death-heal retained building bucket closure changed')
    decoded = [_decode_w3p_string_payload(_decode_lua_short_string_contents(m[1]), 11351, 1106).decode()
               for m in re.finditer(rb'_T\("((?:\\.|[^"\\])*)"\)', target.encode())]
    if len(decoded) != 2 or not decoded[0].endswith('.mdl'):
        raise ValueError('death-heal native recipient model/attachment decode changed')
    return {'source_rawcode': p['linker_unit_id'], 'marker': p['elemental_unit_marker_ability_id'],
            'heal_per_building': p['heal_per_owned_elemental_production_building'],
            'maximum_heal': p['maximum_heal'], 'radius': p['heal_radius'] * 1024,
            'other_type_per_10k': int(Decimal(str(p['other_unit_type_heal_factor'])) * 10000),
            'production_rawcodes': [int(r['building_rawcode_integer']) for r in buckets],
            'model_path': decoded[0], 'attachment': decoded[1]}
