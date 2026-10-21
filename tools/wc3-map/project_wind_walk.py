"""Native Wind Walk and the source-owned shared support order queue."""
import re
from decimal import Decimal


def support_queue_rules(detail):
    """The shared support task queue's rules, identical for every actor kind that joins it."""
    tick = lambda v: int((Decimal(str(v))*30).to_integral_value(rounding='ROUND_CEILING'))
    number = lambda v, scale=1: int(Decimal(str(v))*scale)
    f = detail['functions']
    order, valid, init, queue = (f[k] for k in ['orderAssassinW','isValidAssassinTarget','lE','nH'])
    # Protected Rect dispatch has nested parentheses; collect the four source coordinates.
    def rectangle(symbol,body):
        tail = body.split(symbol+'=_b',1)[1].split(')](',1)[1]
        values = re.match(r'\(?(-?[0-9.]+)\)?,\(?(-?[0-9.]+)\)?,\(?(-?[0-9.]+)\)?,\(?(-?[0-9.]+)\)?',tail)
        if values is None: raise ValueError('support source rectangle changed')
        return [{'x':number(values[1],1024),'y':number(values[2],1024)}, {'x':number(values[3],1024),'y':number(values[4],1024)}]
    return {'map_version':detail['map_version'],'interval_ticks':tick(re.search(r'Gcb=([0-9.]+)',queue)[1]),'budget':int(re.search(r'Fcb=(\d+)',queue)[1]),'cache_ticks':tick(re.search(r'WAb=([0-9.]+)',init)[1]),'retention_limit':int(re.search(r'OX\[n8o\]\)>?(\d+)\)',order)[1]),'preferred_mana_above':number(re.search(r'UNIT_STATE_MAX_MANA\)>([0-9.]+)',f['mE'])[1]),'fallback_health_below':[number(re.search(r'<([0-9.]+)\)',f[k])[1]) for k in ['nE','oE']],'local_rectangles':[rectangle('KAb',init),rectangle('JAb',init)],'battlefield':rectangle('uIb',f['main']),'excluded_ability':int(re.search(r'GetUnitAbilityLevel\(Udo,(\d+)\)',f['isNotTentacle'])[1]),'excluded_buffs':list(map(int,re.findall(r'unit_getAbilityLevel\(o7o,(\d+)\)<=0',valid)))}


def project(recipe, fields, unit, protected, detail):
    tick = lambda v: int((Decimal(str(v))*30).to_integral_value(rounding='ROUND_CEILING'))
    number = lambda v, scale=1: int(Decimal(str(v))*scale)
    raw = lambda v: int.from_bytes(v.encode('ascii'),'big')
    f = detail['functions']
    order, valid, init, queue = (f[k] for k in ['orderAssassinW','isValidAssassinTarget','lE','nH'])
    if detail['native_base'] != 'AOwk' or fields['datad1'] != '1' or fields['datae1'] != '0' or fields['order'] != 'windwalk' or fields['dur1'] != fields['herodur1']:
        raise ValueError('unsupported Wind Walk native timing/backstab variant')
    if 'isCloaked(m8o)or issueAssassinImmediateOrder(m8o,852129)' not in order or 'OX[n8o])+1' not in order or 'OX[n8o]=0' not in order or 'group_getRandom(VAb[p8o])' not in order or 'group_getRandom(SAb[p8o])' not in order:
        raise ValueError('support order selection or retained target control changed')
    if any(x not in valid for x in ['isAliveCombatSapper(', 'unit_isEnemyOf(', 'isVulnerable(', 'isNotTentacle(', 'isNotBanished(', 'not unit_isType(o7o,UNIT_TYPE_FLYING)']):
        raise ValueError('support target predicate changed')
    if 'TaskQueue_TaskQueue_add(Tym,Uym)' not in f['SupportOrderTask_SupportOrderTask_run'] or 'TaskQueue_tasksPerTick*4' not in f['TaskQueue_TaskQueue_executeTasks'] or 'TaskQueue_maximumInterval/2.' not in f['TaskQueue_TaskQueue_executeTasks']:
        raise ValueError('support queue capacity or rescheduling changed')
    royal = raw(recipe['unit_rawcode']) == int(re.search(r'XAb=(\d+)',init)[1])
    threshold = re.search(r'<([0-9.]+)\)',f['oE' if royal else 'nE'])[1]
    mana = lambda k,scale=1: 0 if unit[k] in ['-','_',''] else number(unit[k],scale)
    rate = mana('mana_regen',10000)
    rules = support_queue_rules(detail)
    profile = {'ability':raw(recipe['source_key']),'rules':rules,'fade_ticks':tick(fields['dataa1']),'duration_ticks':tick(fields['dur1']),'movement_percent_delta':number(fields['datab1'],100),'backstab_damage':number(fields['datac1']),'fallback_health_below':number(threshold)}
    return {'kind':'scripted-automatic','source_kind':recipe['source_kind'],'source_key':recipe['source_key'],'unit_rawcode':recipe['unit_rawcode'],'spellcasting':{'mana':{'maximum':mana('mana_max'),'starting':mana('mana_start'),'regen_per_tick_per_10k':(1<<31)|rate},'ability':{'id':raw(recipe['source_key']),'mana_cost':number(protected.get('mana_cost',fields['cost1'])),'cooldown_ticks':tick(protected.get('cooldown',fields['cool1'])),'range':0,'target_policy':'ScriptedController','effect':{'WindWalk':profile}}},'mana_regen_per_second_per_10k':rate,'provenance':{'git_tree':detail['git_tree'],'script':['orderAssassinW','prepareAssassinTargets','isValidAssassinTarget','SupportOrderTask_SupportOrderTask_run','TaskQueue_TaskQueue_executeTasks'],'fields':'resolved/object-fields.tsv'}}
