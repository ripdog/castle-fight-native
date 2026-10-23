"""Native Heal order, actual effect mask, and an independent script resume callback."""
import json
import re
from decimal import Decimal, ROUND_CEILING

HEAL_MASK = {'air','friend','ground','invulnerable','neutral','nonancient','organic','self','vulnerable'}
SPRAY_MASK = {'air', 'friend', 'ground'}
SPRAY_ORDER = 'healingspray'
FULL_NAMES = {'vuln': 'vulnerable', 'invu': 'invulnerable'}


def tokens_of(value):
    return {FULL_NAMES.get(token, token) for token in value.split(',')}


def _spray(recipe, detail, handler, delay, tick):
    """Panda Monk Heal: the script sprays the healed unit's position from a free dummy at the caster."""
    semantics = detail['semantics_row']
    child = detail['effect_key']
    fields = detail['effect_fields']
    protected = detail['effect_protected']
    p = json.loads(semantics['parameters_json'])
    sites = p['delivery_sites']
    if (detail['all_bases'][child] != 'ANhs' or semantics['semantic_kind'] != 'proxy-object-effect'
        or len(sites) != 1 or sites[0]['callee'] != 'dummyCastPointFrom' or sites[0]['effect_rawcodes'] != [child]
        or tokens_of(fields['targs1']) != SPRAY_MASK
        or Decimal(protected.get('mana_cost', fields['cost1'])) != 0 or Decimal(fields['cool1']) != 0):
        raise ValueError('Healing Spray requires a free native friendly dummy channel')
    shape = (r'function \w+\((\w+),(\w+),(\w+)\)(?:local \w+=nil )+(\w+)=\2 (\w+)=\3 (\w+)=\4 '
        r'dummyCastPointFrom\(unit_getOwner\(\6\),(\d+),OrderId\("' + SPRAY_ORDER + r'"\),'
        r'unit_getPos\(\5\),unit_getPos\(\6\),([0-9.]+)\)(\w+)=\w+:create\d+\(\)\9\.u=\6 doAfter\(([0-9.]+),\9\)end')
    matched = re.fullmatch(shape, handler.strip())
    if matched is None or int(matched[7]) != int.from_bytes(child.encode(), 'big') or Decimal(matched[10]) != Decimal(delay):
        raise ValueError('Healing Spray must be cast from the caster at the healed unit with the retained resume delay')
    tokens = tokens_of(fields['targs1'])
    return {'ability': int.from_bytes(child.encode(), 'big'), 'radius': int(Decimal(fields['area1']) * 1024),
        'amount': int(Decimal(fields['dataa1'])), 'maximum_total': int(Decimal(fields['datad1'])),
        'waves': int(Decimal(fields['dataf1'])), 'interval_ticks': tick(fields['datab1']),
        # Deterministic interpretation: the first wave lands one wave interval after the cast.
        'first_wave_ticks': tick(fields['datab1']), 'lifetime_ticks': tick(matched[8]),
        'targets': (1 if 'ground' in tokens else 0) | (2 if 'air' in tokens else 0)}


REJUVENATION_MASK = {'air', 'friend', 'ground', 'invulnerable', 'neutral', 'organic', 'self', 'vulnerable'}


def _rejuvenation(recipe, detail, handler, delay, tick):
    """Furbolg Shaman: the heal order triggers a free dummy-cast native Rejuvenation on the same unit."""
    semantics = detail['semantics_row']
    child, fields, bases = detail['effect_key'], detail['effect_fields'], detail['all_bases']
    p = json.loads(semantics['parameters_json'])
    sites = p['delivery_sites']
    shape = (r'function \w+\((\w+),(\w+),(\w+)\)(?:local \w+=nil )+(\w+)=\2 (\w+)=\3 (\w+)=unit_getOwner\(\4\)(\w+)=\4 (\w+)=\5 '
        r'dummyCastTargetFrom1\(\6,(\d+),(\d+),\8,unit_getPos\(\7\),([0-9.]+)\)(\w+)=\w+:create\d+\(\)\12\.caster=\4 doAfter\(([0-9.]+),\12\)end')
    matched = re.fullmatch(shape, handler.strip())
    if (semantics['semantic_kind'] != 'proxy-object-effect' or len(sites) != 1 or bases[child] != 'Arej'
        or sites[0]['callee'] != 'dummyCastTargetFrom1' or sites[0]['effect_rawcodes'] != [child]
        or tokens_of(fields['targs1']) != REJUVENATION_MASK or len(fields['buffid1']) != 4
        or Decimal(fields['datab1']) != 0 or Decimal(fields['dataa1']) <= 0
        or Decimal(fields['dur1']) <= 0 or Decimal(fields['herodur1']) <= 0
        or matched is None or int(matched[9]) != int.from_bytes(child.encode(), 'big')
        or Decimal(matched[13]) != Decimal(delay)):
        raise ValueError('Rejuvenation requires a free dummy-cast native Arej at the healed unit with the retained resume delay')
    raw = lambda code: int.from_bytes(code.encode(), 'big')
    amount, seconds = Decimal(fields['dataa1']), Decimal(fields['dur1'])
    if Decimal(fields['herodur1']) != seconds:
        raise ValueError('Rejuvenation heals its retained amount over one lifetime for every recipient')
    regeneration = amount * 10_000 / seconds
    if regeneration != regeneration.to_integral_value():
        raise ValueError('Rejuvenation regeneration is not exactly representable')
    tokens = tokens_of(fields['targs1'])
    return {'ability': raw(child),
        'buff': {'rawcode': raw(fields['buffid1']), 'positive': True, 'stealable': fields.get('hero', '0') == '0',
            'organic_only': 'organic' in tokens},
        'regeneration_per_second_per_10k': int(regeneration), 'duration_ticks': tick(fields['dur1']),
        'hero_duration_ticks': tick(fields['herodur1']), 'organic_only': 'organic' in tokens}


TOUCH_KINDS = {'registered-heal-plus-permanent-armor', 'registered-heal-armor-plus-summon'}


def _touch(recipe, detail, handler, delay, tick):
    """Nature's Touch: the heal order permanently adds an item armor ability to its target, then resumes."""
    semantics, row, functions = detail['semantics_row'], detail['mechanics_row'], detail['functions']
    p = json.loads(semantics['parameters_json'])
    armor = p['permanent_armor_ability_rawcode']
    shape = (r'addProtectedAbility\((\w+),(\d+)\)\w+=addEffect1\("[^"]+",\w+,"overhead"\)__wurst_safe_DestroyEffect\(\w+\)'
        r'\w+=\w+:create\d+\(\)\w+\.caster=\w+ doAfter\(([0-9.]+),\w+\)end$')
    matched = re.search(shape, handler.strip())
    callbacks = [functions[name] for name in row['delayed_callback_functions'].split(',')]
    resume = r'function \w+\((\w+)\)orderCodeAttack\(\1\.caster\)end'
    if (matched is None or int(matched[2]) != int.from_bytes(armor.encode(), 'big')
            or Decimal(matched[3]) != Decimal(delay) or row['mechanic_kind'] != 'apply-protected-ability'
            or detail['all_bases'].get(armor, armor) != armor):
        raise ValueError('Nature\'s Touch requires the permanent armor grant at the healed unit and the retained resume delay')
    bonus = Decimal(str(p['permanent_armor_bonus']))
    grant = {'ability': int.from_bytes(armor.encode(), 'big'), 'effect': {'Armor': int(bonus * 100)}}
    summon = None
    if semantics['semantic_kind'] == 'registered-heal-armor-plus-summon':
        order = re.fullmatch(r'function \w+\((\w+)\)local \w+=nil if unit_issueImmediateOrderById\(\1\.caster,(\d+)\)then \w+=\w+:create\d+\(\)'
            r'\w+\.caster=\1\.caster doAfter\(([0-9.]+),\w+\)else orderCodeAttack\(\1\.caster\)end end', callbacks[0].strip())
        if (len(callbacks) != 2 or order is None or int(order[2]) != p['summon_order_id']
                or Decimal(order[3]) != Decimal(str(p['resume_attack_after_summon_seconds']))
                or Decimal(delay) != Decimal(str(p['post_heal_delay_seconds']))
                or re.fullmatch(resume, callbacks[1].strip()) is None):
            raise ValueError('Keeper\'s Nature\'s Touch must attempt its Spirit Wolf order and then resume')
        summon = {'ability': int.from_bytes(p['summon_ability_rawcode'].encode(), 'big'),
            'resume_delay_ticks': tick(order[3])}
    elif len(callbacks) != 1 or re.fullmatch(resume, callbacks[0].strip()) is None or Decimal(delay) != Decimal(str(p['resume_attack_delay_seconds'])):
        raise ValueError('Nature\'s Touch must resume the retained caster identity')
    return {'armor': grant, 'summon': summon}


def project(recipe, fields, unit, protected, detail):
    row, semantics = detail['mechanics_row'], detail['semantics_row']
    p = json.loads(semantics['parameters_json'])
    touching = semantics['semantic_kind'] in TOUCH_KINDS
    rejuvenating = (semantics['semantic_kind'] == 'proxy-object-effect'
        and row['mechanic_kind'] == 'dummy-target-ability-from-owner')
    scripted_spray = semantics['semantic_kind'] == 'proxy-object-effect' and not rejuvenating
    if (detail['native_base'] != 'Ahea' or row['base_order'] != 'heal'
        or tokens_of(fields['targs1']) != HEAL_MASK):
        raise ValueError('Heal requires distinct native friendly effect and script trigger')
    if scripted_spray:
        if row['target_mode_label'] != 'ally-any' or row['mechanic_kind'] != 'dummy-point-ability':
            raise ValueError('Heal spray requires the any-ally dummy point dispatch')
    elif touching:
        if row['target_mode_label'] != 'ally-ground':
            raise ValueError('Nature\'s Touch requires the ground-ally heal trigger')
    elif rejuvenating:
        if row['target_mode_label'] != 'ally-ground':
            raise ValueError('Rejuvenation requires the ground-ally heal trigger')
    elif (row['target_mode_label'] != 'ally-ground' or row['mechanic_kind'] != 'scheduled-callback'
        or semantics['semantic_kind'] != 'registered-object-effect-plus-resume'
        or p['primary_effect_ability_rawcode'] != recipe['source_key']):
        raise ValueError('Heal requires distinct native friendly effect and ground script trigger')
    delays = json.loads(row['scheduled_delays_json'])
    if len(delays) != 1 or (not scripted_spray and not rejuvenating and not touching
            and Decimal(delays[0]) != Decimal(str(p['resume_attack_delay_seconds']))):
        raise ValueError('Heal resume deadline differs from retained callback')
    functions = detail['functions']
    handler = functions[row['handler_function']]
    tick = lambda value: int((Decimal(str(value))*30).to_integral_value(rounding=ROUND_CEILING))
    spray = None
    rejuvenation = None
    touch = None
    if touching:
        touch = _touch(recipe, detail, handler, delays[0], tick)
    elif rejuvenating:
        callback = functions[row['delayed_callback_functions'].split(',')[0]]
        if not re.fullmatch(r'function \w+\((\w+)\)orderCodeAttack\(\1\.caster\)end', callback.strip()):
            raise ValueError('Rejuvenation must resume the retained caster identity')
        rejuvenation = _rejuvenation(recipe, detail, handler, delays[0], tick)
    elif scripted_spray:
        callback = functions[row['delayed_callback_functions'].split(',')[0]]
        if not re.fullmatch(r'function \w+\((\w+)\)if\(\(not\(\1\.u==nil\)\)and\(widget_getLife\(\1\.u\)>0\.405\)\)then orderCodeAttack\(\1\.u\)end end', callback.strip()):
            raise ValueError('Heal spray must resume the retained caster identity')
        spray = _spray(recipe, detail, handler, delays[0], tick)
    else:
        callback = functions['CallbackSingle_doAfter_RaceNelfAbilities_call_doAfter_RaceNelfAbilities1']
        if ('Qwn.u=Pwn' not in handler or f'doAfter({delays[0]},Qwn)' not in handler
            or 'orderCodeAttack(jwn.u)' not in callback):
            raise ValueError('Heal must retain caster identity and delayed attack order')
    raw = int.from_bytes(recipe['source_key'].encode(), 'big')
    mana = lambda key, scale=1: 0 if unit[key] in {'','-','_'} else int(Decimal(unit[key])*scale)
    rate = mana('mana_regen', 10000)
    trigger_amount = Decimal(fields['dataa1'])
    heal = {'amount':int(trigger_amount),'targets':3,'organic_only':True,'nonancient_only':True,
        'affects_invulnerable':True,'resume_attack_ticks':tick(delays[0])}
    if spray is not None:
        heal['spray'] = spray
    if rejuvenation is not None:
        heal['rejuvenation'] = rejuvenation
    if touch is not None:
        heal['touch'] = touch
    return {'kind':'scripted-automatic','source_kind':recipe['source_kind'],'source_key':recipe['source_key'],'unit_rawcode':recipe['unit_rawcode'],
        'spellcasting':{'mana':{'maximum':mana('mana_max'),'starting':mana('mana_start'),'regen_per_tick_per_10k':(1<<31)|rate},
        'ability':{'id':raw,'mana_cost':int(protected.get('mana_cost',fields['cost1'])), 'cooldown_ticks':tick(protected.get('cooldown',fields['cool1'])),
        'range':int(Decimal(fields['rng1'])*1024),'target_policy':'WoundedFriendlyUnit','effect':{'Heal':heal}}}, 'mana_regen_per_second_per_10k':rate,
        'provenance':{'unrepresented_trigger_heal':str(trigger_amount - int(trigger_amount)),'git_tree':detail['git_tree'],'native':'resolved/object-fields.tsv','script':'resolved/unit-spell-mechanics.tsv','semantics':'resolved/unit-spell-semantics.tsv'}}
