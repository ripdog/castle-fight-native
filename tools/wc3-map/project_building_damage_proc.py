"""Construction-granted building damage callbacks from retained runtime evidence."""
import json,re
from decimal import Decimal,ROUND_CEILING


def project(row, fields, protected, traces):
    params=json.loads(row['parameters_json'])
    source=params['source_unit_id']; marker=int.from_bytes(b'A03Q','big')
    helper=params['tooltip_helper_ability_id']; child=params['effect_ability_id']
    grant=traces['iD']['source'];dispatch=traces['handleSourceDamageEffects']['source'];proc=traces['boulderAttackProc']['source']
    if f'mho=={source})then safeUnitAddAbility(jho,{marker})safeUnitAddAbility(jho,{helper})' not in grant:
        raise ValueError('building damage proc requires both exact construction grants')
    if f'unit_getAbilityLevel(Qpq,{marker})<=0' not in dispatch or f'Spq=={source})then boulderAttackProc(Qpq,Rpq)' not in dispatch:
        raise ValueError('building damage callback must preserve type and dispatcher gates')
    roll=re.search(r'GetRandomInt\(0,99\)<(\d+)',proc)
    life=re.search(r'UnitApplyTimedLife\(Hoq,1112820806,([\d.]+)\)',proc)
    if (not roll or not life or int(roll[1])!=params['proc_probability_percent']
        or Decimal(life[1])!=Decimal(str(params['dummy_timed_life_seconds']))
        or f'addProtectedAbility(Ioq,{child})' not in proc
        or f'unit_issueTargetOrderById(Hoq,{params["target_order_id"]},Eoq)' not in proc
        or 'unit_getPos(Doq)' not in proc or 'CreateUnit(Foq,dV' not in proc):
        raise ValueError('building damage callback random threshold, origin, child or lifetime drift')
    code=child.to_bytes(4,'big').decode();f=fields[code]
    value=lambda name:Decimal(str(f[name]))
    ticks=lambda v:int((Decimal(str(v))*30).to_integral_value(rounding=ROUND_CEILING))
    runtime={r['field']:r['runtime_value'] for r in protected if r['rawcode']==code and r['level']=='1'}
    if Decimal(runtime['mana_cost'])!=0 or f['atar:1']!='air,enemies,ground':
        raise ValueError('boulder requires a free independently restricted native unit order')
    if int(value('Ctb1:1'))!=params['effect_damage'] or ticks(value('adur:1'))!=ticks(params['effect_duration_normal_seconds']) or ticks(value('ahdu:1'))!=ticks(params['effect_duration_hero_seconds']):
        raise ValueError('boulder runtime summary differs from native child object')
    return {'source_rawcode':source,'helper':helper,'dispatcher':marker,
        'chance_per_10k':int(roll[1])*100,'range':int(value('aran:1')*1024),
        'dummy_lifetime_ticks':ticks(life[1]),'child_cooldown_ticks':ticks(runtime['cooldown']),
        'bolt':{'ability':child,'damage':int(value('Ctb1:1')),'stun_ticks':ticks(value('adur:1')),
            'hero_stun_ticks':ticks(value('ahdu:1')),'damage_per_second':0,'duration_ticks':0,
            'speed_per_tick':int(value('amsp:0')*1024/30),'cleanse':False,'targets':3}}
