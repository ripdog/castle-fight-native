"""Native fJ death/kill callbacks, with retained object identity and execution order."""
import json
import re


def project(rows, functions):
    source = functions['fJ']
    if ('if isCombatSapper(M2q)then' not in source
        or 'if(not(N2q==nil))then if(not unit_isType(N2q,UNIT_TYPE_STRUCTURE))then' not in source):
        raise ValueError('native death callback eligibility changed')
    result = {}
    for row in rows:
        kind = row['mechanic_kind']
        if kind not in {'kill-heal-percent-max-hp', 'death-retaliation-damage-to-killer'}:
            continue
        p = json.loads(row['parameters_json'])
        raw = int.from_bytes(row['unit_rawcode'].encode(), 'big')
        value = result.setdefault(raw, {'rawcode': raw, 'heal_percent': 0, 'retaliation_damage': 0})
        if kind == 'kill-heal-percent-max-hp':
            if (p['implementation'] != 'set-life-percent-current-plus-delta'
                or not p['victim_requires_combat_sapper'] or not p['killer_must_not_be_structure']
                or not re.search(r'R2q=='+str(raw)+r'\)then.*?GetUnitLifePercent\(N2q\)\+'+str(p['heal_percent_of_max_hp'])+r'\.', source)):
                raise ValueError('native kill-heal ownership/formula changed')
            value['heal_percent'] = p['heal_percent_of_max_hp']
        else:
            if (p['damage_source'] != 'dying-unit' or p['damage_target'] != 'killer'
                or p['attack_type'] != 'chaos' or p['damage_type'] != 'normal' or not p['is_attack']
                or p['is_ranged'] or not p['dying_unit_requires_combat_sapper']
                or not p['killer_required'] or not p['killer_must_not_be_structure']):
                raise ValueError('unsupported native death retaliation shape')
            match = re.search(r'if\(unit_getTypeId\(M2q\)=='+str(raw)+r'\)then __wurst_safe_UnitDamageTarget\(M2q,N2q,([\d.]+),true,false,ATTACK_TYPE_CHAOS,DAMAGE_TYPE_NORMAL,WEAPON_TYPE_WHOKNOWS\)end R2q=unit_getTypeId\(N2q\)', source)
            if not match or float(match[1]) != p['damage']:
                raise ValueError('native retaliation must precede killer heal/type branch')
            value['retaliation_damage'] = p['damage']
    return [result[k] for k in sorted(result)]
