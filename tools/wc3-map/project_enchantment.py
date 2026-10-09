"""Permanent Elemental enchantments from native children and exact retained callbacks."""
import json
import re


def grant(source, fields, bases, functions, scaled):
    raw = lambda code: int.from_bytes(code.encode(), 'big')
    f, base = fields[source], bases[source]
    dependencies = []
    if base == 'AOae':
        if f['targs1'] != 'self' or scaled(f['area1']) != 0:
            raise ValueError('enchantment haste must remain a self-only native aura')
        effect = {'Speed': {'attack': scaled(f['datab1'],100), 'movement': scaled(f['dataa1'],100), 'buff_rawcode': raw(f['buffid1'])}}
    elif base == 'Aspb':
        children = f['dataa1'].split(',')
        if len(children) != 2 or scaled(f['datab1']) != 1 or any(scaled(f[k]) != 2 for k in ['datac1','datad1']):
            raise ValueError('rock shield requires both native Spell Book children')
        armor, badge = (fields[code] for code in children)
        if bases[children[0]] != 'AId4' or bases[children[1]] != 'Aasl' or any(scaled(badge[k]) != 0 for k in ['dataa1','datab1']) or badge['targs1'] != 'self':
            raise ValueError('rock shield child payload changed')
        effect = {'SpellBookArmor': {'bonus_per_100': scaled(armor['dataa1'],100), 'child':raw(children[0]), 'badge':raw(children[1])}}
        dependencies = children
    elif base == 'AId4':
        effect = {'Armor':scaled(f['dataa1'],100)}
    elif base == 'Aasl':
        if any(scaled(f[k]) != 0 for k in ['dataa1','datab1']) or f['targs1'] != 'self':
            raise ValueError('enchantment marker aura must have no native slow payload')
        if source not in functions['handleSourceDamageEffects'] and str(raw(source)) not in functions['handleSourceDamageEffects']:
            effect = 'AuraMarker'
        else:
            body = functions['handleSourceDamageEffects']
            shape = re.search(r'unit_getAbilityLevel\(Qpq,(\d+)\)<=0.*?unit_getAbilityLevel\(Qpq,'+str(raw(source))+r'\)>0\)then Tpq=\(([0-9.]+)\*GetEventDamage\(\)\).*?forUnitsInRange\(bqq,([0-9.]+),false,cqq\)',body)
            callback = functions['ForGroupCallback_forUnitsInRange_DamageRuntime_callback_forUnitsInRange_DamageRuntime']
            target = functions['cleaveHitsTarget']
            if (not shape or '__wurst_safe_UnitDamageTarget(W9l.dummy,X9l,W9l.damage,true,false,ATTACK_TYPE_CHAOS,DAMAGE_TYPE_UNKNOWN' not in callback
                or 'cleaveHitsTarget(W9l.source,X9l,W9l.hitsGround,W9l.hitsFlying)' not in callback
                or not all(token in body for token in ['unit_getX(Rpq)', 'unit_getY(Rpq)', 'UNIT_TYPE_ATTACKS_GROUND', 'UNIT_TYPE_ATTACKS_FLYING', '__wurst_safe_CreateUnit(unit_getOwner(Qpq),1697656888', '__wurst_safe_UnitApplyTimedLife(Ypq,1112820806,1.)'])
                or not all(token in target for token in ['unit_isEnemyOf','isCombatSapper','isVulnerable','UNIT_TYPE_GROUND','UNIT_TYPE_FLYING'])
                or 'SPELL_IMMUNE' in target):
                raise ValueError('forge weapon requires exact source dispatcher, dummy damage and target semantics')
            effect = {'SourceDamageSplash': {'dispatcher':int(shape[1]), 'damage_per_10k':scaled(shape[2],10_000), 'radius':scaled(shape[3],1024)}}
    else:
        raise ValueError('unsupported native enchantment child')
    return {'ability':raw(source), 'effect':effect}, dependencies


def project(recipe, fields, unit, protected, mechanics, scaled):
    kind = recipe['kind']
    if kind == 'source-damage-dispatcher':
        ability = int.from_bytes(recipe['source_key'].encode(),'big')
        if (f'unit_getAbilityLevel(Qpq,{ability})<=0' not in mechanics['functions']['handleSourceDamageEffects']
            or mechanics['native_base'] != 'ANcl' or any(scaled(fields[k]) != 0 for k in ['datab1', 'datac1'])):
            raise ValueError('source dispatcher identity changed')
        return dict(recipe, ability=ability)
    if kind == 'native-enchantment-grant':
        payload, dependencies = grant(recipe['source_key'], mechanics['all_fields'], mechanics['all_bases'], mechanics['functions'],scaled)
        return dict(recipe, grant=payload, dependencies=dependencies)
    row = mechanics['building_spell']
    params = json.loads(row['parameters_json'])
    body = mechanics['functions']['holyShrineSpell']
    candidates = params['candidate_ability_ids']
    marker = params['always_after-buff_ability_id']
    dispatcher = params['generated_selected-index-2_extra_ability_id']
    if (row['mechanic_kind'] != 'heal-or-random-missing-buff'
        or not all(token in body for token in ['holyPick(Npr,false)','holyPick(Npr,true)',
            'Rpr=(Rpr-1)Qpr[Spr]=__wurst_ensureInt(Qpr[Rpr])','Spr=GetRandomInt(0,(Rpr-1))',
            f'(unit_getMaxHP(Opr)-Ppr)>{params["heal_if_missing_hp_gt"]}.',
            f'(Ppr+{params["heal_amount"]}.)',f'addProtectedAbility(Xpr,{dispatcher})', f'addProtectedAbility(Ypr,{marker})'])
        or [int(x) for x in re.findall(r'Qpr\[\d+\]=(\d+)',body)] != candidates
        or not re.search(r'if\(Spr==2\)then',body)):
        raise ValueError('enchantment requires exact candidate swap removal, heal branch and indexed dispatcher grant')
    # Keep the marker inside the successful buff branch; healing leaves it absent.
    if not re.search(r'elseif\(widget_getLife\(Opr\)>0\.405\)then.*?addProtectedAbility\(Ypr,'+str(marker)+r'\)end end noteActive', body):
        raise ValueError('enchantment retry marker must be confined to the buff branch')
    selector = mechanics['functions']['isHolyShrineTarget']
    if not all(x in selector for x in ['isAliveCombatSapper','unit_isAllyOf', 'unit_getAbilityLevel(ypr,1098282348)<=0', f'unit_getAbilityLevel(ypr,{marker})<=0']):
        raise ValueError('enchantment target preference changed')
    grants = [grant(int(code).to_bytes(4,'big').decode(), mechanics['all_fields'],mechanics['all_bases'],mechanics['functions'],scaled)[0] for code in candidates]
    raw = lambda code: int.from_bytes(code.encode(),'big')
    tokens = set(fields['targs1'].split(','))
    if (tokens != {'enemies','ground','invulnerable','mechanical','neutral','structure'}
        or mechanics['native_base'] != 'ACpa'
        or any(scaled(fields[k]) != 0 for k in ['dataa1','datab1','datac1','datad1'])):
        raise ValueError('native Parasite trigger differs from enchantment effect picker')
    rate = scaled(unit['mana_regen'],10_000)
    profile = {'map_version':mechanics['map_version'],'trigger_targets':6,'trigger_invulnerable':True,'trigger_spell_immune':False,
        'heal_if_missing_hp_gt':params['heal_if_missing_hp_gt'],'heal_amount':params['heal_amount'],
        'retry_marker':marker,'dispatcher':dispatcher,'dispatcher_option_index':2,'options':grants}
    return {'kind':'scripted-automatic','source_kind':recipe['source_kind'],'source_key':recipe['source_key'],'unit_rawcode':recipe['unit_rawcode'],
        'spellcasting':{'mana':{'maximum':scaled(unit['mana_max']), 'starting':0 if unit['mana_start'].strip() in {'-','_'} else scaled(unit['mana_start']), 'regen_per_tick_per_10k':(1<<31)|rate},
            'ability':{'id':raw(recipe['source_key']),'mana_cost':scaled(protected['mana_cost']), 'cooldown_ticks':scaled(protected['cooldown'],30), 'range':scaled(fields['rng1'],1024),
                'target_policy':'NativeBuildingSpellTrigger','effect':{'BuildingEnchantment':profile}}},
        'mana_regen_per_second_per_10k':rate,'effect_ability_rawcode':None,'dependencies':[int(code).to_bytes(4,'big').decode() for code in candidates]+[int(marker).to_bytes(4,'big').decode()],
        'provenance':{'git_tree':mechanics['git_tree'],'source':'resolved/building-spell-mechanics.tsv','functions':'holyShrineSpell,holyPick,isHolyShrineTarget,handleSourceDamageEffects,cleaveHitsTarget'}}
