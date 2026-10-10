"""Native Heal order, actual effect mask, and an independent script resume callback."""
import json
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, unit, protected, detail):
    row, semantics = detail['mechanics_row'], detail['semantics_row']
    p = json.loads(semantics['parameters_json'])
    if (detail['native_base'] != 'Ahea' or row['target_mode_label'] != 'ally-ground'
        or row['base_order'] != 'heal' or row['mechanic_kind'] != 'scheduled-callback'
        or semantics['semantic_kind'] != 'registered-object-effect-plus-resume'
        or p['primary_effect_ability_rawcode'] != recipe['source_key']
        or set(fields['targs1'].split(',')) != {'air','friend','ground','invulnerable','neutral','nonancient','organic','self','vulnerable'}):
        raise ValueError('Heal requires distinct native friendly effect and ground script trigger')
    delays = json.loads(row['scheduled_delays_json'])
    if len(delays) != 1 or Decimal(delays[0]) != Decimal(str(p['resume_attack_delay_seconds'])):
        raise ValueError('Heal resume deadline differs from retained callback')
    functions = detail['functions']
    handler = functions[row['handler_function']]
    callback = functions['CallbackSingle_doAfter_RaceNelfAbilities_call_doAfter_RaceNelfAbilities1']
    if ('Qwn.u=Pwn' not in handler or f'doAfter({delays[0]},Qwn)' not in handler
        or 'orderCodeAttack(jwn.u)' not in callback):
        raise ValueError('Heal must retain caster identity and delayed attack order')
    raw = int.from_bytes(recipe['source_key'].encode(), 'big')
    tick = lambda value: int((Decimal(str(value))*30).to_integral_value(rounding=ROUND_CEILING))
    mana = lambda key, scale=1: 0 if unit[key] in {'','-','_'} else int(Decimal(unit[key])*scale)
    rate = mana('mana_regen', 10000)
    return {'kind':'scripted-automatic','source_kind':recipe['source_kind'],'source_key':recipe['source_key'],'unit_rawcode':recipe['unit_rawcode'],
        'spellcasting':{'mana':{'maximum':mana('mana_max'),'starting':mana('mana_start'),'regen_per_tick_per_10k':(1<<31)|rate},
        'ability':{'id':raw,'mana_cost':int(protected.get('mana_cost',fields['cost1'])), 'cooldown_ticks':tick(protected.get('cooldown',fields['cool1'])),
        'range':int(Decimal(fields['rng1'])*1024),'target_policy':'WoundedFriendlyUnit','effect':{'Heal':{
            'amount':int(fields['dataa1']),'targets':3,'organic_only':True,'nonancient_only':True,'affects_invulnerable':True,
            'resume_attack_ticks':tick(delays[0])}}}}, 'mana_regen_per_second_per_10k':rate,
        'provenance':{'git_tree':detail['git_tree'],'native':'resolved/object-fields.tsv','script':'resolved/unit-spell-mechanics.tsv','semantics':'resolved/unit-spell-semantics.tsv'}}
