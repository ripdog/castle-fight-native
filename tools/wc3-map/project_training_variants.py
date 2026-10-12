"""Native training replaces its handle, then rolls independent permanent ability groups."""
import re


def project(functions, units, bases):
    trained = functions['onUnitTrained']
    body = functions['rollBody']
    initialize = functions['EE']
    grants = functions['randomizeBloodFiend']
    replace = functions['replaceUnitWithType']
    marker = re.search(r'if\(unit_getAbilityLevel\(cgs,(\d+)\)>0\)then cgs=randomizeBloodFiend\(cgs\)elseif', trained)
    selector = re.search(r'A_o=GetRandomInt\((\d+),(\d+)\)if\(A_o<(\d+)\)then return replaceUnitWithType\(z_o,qAb\)elseif\(A_o<(\d+)\)then return replaceUnitWithType\(z_o,pAb\)end return replaceUnitWithType\(z_o,__wurst_ensureInt\(oAb\[GetRandomInt\((\d+),(\d+)\)\]\)\)', body)
    rare = re.search(r'qAb=(\d+) pAb=(\d+)', initialize)
    common = re.findall(r'oAb\[(\d+)\]=(\d+)', initialize)
    if (not all([marker, selector, rare]) or not common
        or 'C_o=rollBody(B_o)' not in grants or 'return C_o' not in grants
        or 'pFb=u_o' not in replace
        or '__wurst_safe_ShowUnit(x_o,false)' not in replace
        or 'CreateUnit(unit_getOwner(u_o),v_o,unit_getX(u_o),unit_getY(u_o),angle_degrees(unit_getFacingAngle(u_o)))' not in replace
        or 'y_o=u_o __wurst_safe_RemoveUnit(y_o)' not in replace
        or 'KillUnit(' in replace or 'setupUnit(' in replace):
        raise ValueError('training replacement ownership, handle removal, cold body selection or grants changed')
    groups = []
    recognized = []
    group_pattern = r'([D-H]_o)=GetRandomInt\((\d+),(\d+)\)(if\(\1<.*?end)'
    for match in re.finditer(group_pattern, grants):
        recognized.append((match.start(), match[0]))
        choices = re.findall(r'(?:if|elseif)\('+match[1]+r'<(\d+)\)then (\w+)=C_o addProtectedAbility\(\2,(\d+)\)', match[4])
        if not choices or match[4].count('addProtectedAbility(') != len(choices):
            raise ValueError('training grant group must add only to the replacement')
        groups.append({'minimum': int(match[2]), 'maximum': int(match[3]),
            'choices': [{'threshold_exclusive': int(cutoff), 'ability': int(ability)} for cutoff, _, ability in choices]})
    extra = re.search(r'if\(GetRandomInt\((\d+),(\d+)\)<(\d+)\)then (\w+)=C_o addProtectedAbility\(\4,(\d+)\)end', grants)
    if len(groups) != 5 or extra is None:
        raise ValueError('training requires every independent conditional grant roll')
    recognized.append((extra.start(), extra[0]))
    sequence = grants.split('C_o=rollBody(B_o)', 1)[1].split('return C_o', 1)[0]
    compact = lambda value: re.sub(r'\s+', '', value)
    if compact(sequence) != compact(''.join(block for _, block in sorted(recognized))):
        raise ValueError('unresolved native training control flow between body replacement and return')
    groups.insert(3, {'minimum': int(extra[1]), 'maximum': int(extra[2]),
        'choices': [{'threshold_exclusive': int(extra[3]), 'ability': int(extra[5])}]})
    abilities = sorted({c['ability'] for g in groups for c in g['choices']})
    if grants.count('addProtectedAbility(') != sum(len(g['choices']) for g in groups):
        raise ValueError('unresolved training ability grant')
    for group in groups:
        cutoffs = [c['threshold_exclusive'] for c in group['choices']]
        if not group['minimum'] < cutoffs[0] or cutoffs != sorted(set(cutoffs)) or cutoffs[-1] > group['maximum'] + 1:
            raise ValueError('training grant thresholds must be ordered inside their native roll range')
    common = [(int(index), int(code)) for index, code in common]
    if [index for index, _ in common] != list(range(int(selector[5]), int(selector[6]) + 1)):
        raise ValueError('training fallback choices must exactly cover the native indexed roll')
    raw = lambda code: int(code).to_bytes(4, 'big').decode('ascii')
    marker_code = raw(marker[1])
    roots = sorted(int.from_bytes(code.encode(), 'big') for code, unit in units.items()
        if marker_code in unit['abilities'].split(','))
    bodies = [int(rare[1]), int(rare[2]), *[code for _, code in common]]
    if not roots or any(code not in units or units[code]['abilities'] for code in map(raw, bodies)):
        raise ValueError('training replacement requires retained cold bodies with their exact empty inventories')
    return {'marker': int(marker[1]), 'roots': roots,
        'body_minimum': int(selector[1]), 'body_maximum': int(selector[2]),
        'rare_bodies': [{'threshold_exclusive': int(selector[i]), 'body': int(rare[i - 2])} for i in [3, 4]],
        'common_minimum': int(selector[5]), 'common_maximum': int(selector[6]),
        'common_bodies': [code for _, code in common], 'grant_groups': groups,
        'grant_sources': [{'kind': 'ability-effect' if bases[raw(code)] == 'ANde' else 'unit-ability', 'key': code} for code in abilities],
        'source_functions': ['onUnitTrained', 'rollBody', 'EE', 'randomizeBloodFiend', 'replaceUnitWithType']}
