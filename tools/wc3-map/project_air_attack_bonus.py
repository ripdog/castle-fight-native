"""Reduced damage-instance rewriting, independently for every flying recipient."""
import json
import re
from decimal import Decimal


def project(rows, functions):
    result = []
    for row in rows:
        if row['mechanic_kind'] != 'attack-damage-flying-target-multiplier':
            continue
        trace = json.loads(row['parameters_json'])
        raw = int.from_bytes(row['unit_rawcode'].encode('ascii'), 'big')
        registration = functions['hL']
        binding = re.search(r' u0=([0-9]+) q0=([0-9.]+) ', registration)
        if (binding is None or int(binding[1]) != raw
                or 'TDr.DamageListener_unreduced=false' not in registration
                or 'DamageEvent_addListener(TDr)' not in registration
                or 'doq.DamageListener_priority=coq' not in functions['DamageEvent_addListener1']
                or 'return DamageEvent_addListener1(0,boq)' not in functions['DamageEvent_addListener']):
            raise ValueError('flying attack bonus requires its reduced priority-zero listener')
        if functions['shouldApplyRiptideAirBonus'].strip() != (
                'function shouldApplyRiptideAirBonus(UDr,VDr,WDr,XDr)'
                'return((((UDr==u0)and VDr)and(WDr==0))and(XDr>0.))end'):
            raise ValueError('flying attack bonus source/type/positive-damage guards changed')
        if functions['addRiptideAirDamageBonus'].strip() != (
                'function addRiptideAirDamageBonus(YDr)return(YDr+(YDr*q0))end'):
            raise ValueError('flying attack bonus must rewrite the current amount once')
        callback = functions['DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack']
        expected_callback = (
            'function DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack(qCn)'
            'local rCn=nil local sCn=nil local tCn=nil local uCn=nil '
            'rCn=DamageEvent_getSource()sCn=DamageEvent_getTarget()tCn=DamageEvent_getAmount()'
            'if((not(rCn==nil))and(not(sCn==nil)))then if '
            'shouldApplyRiptideAirBonus(unit_getTypeId(rCn),unit_isType(sCn,UNIT_TYPE_FLYING),DamageEvent_getType(),tCn)'
            'then uCn=addRiptideAirDamageBonus(tCn)DamageInstance_DamageInstance_setAmount(khb,uCn)end end end')
        if callback.split(' KLb=', 1)[0].strip() != expected_callback:
            raise ValueError('flying bonus must apply to any qualifying current recipient')
        if not all(trace[key] is True for key in (
                'applies_to_any_qualifying_damage_recipient', 'modifies_current_damage_instance',
                'requires_positive_damage', 'target_requires_flying')) or trace['required_damage_event_type'] != 0:
            raise ValueError('flying attack bonus projection disagrees with its event guards')
        bonus = Decimal(binding[2])
        if bonus != Decimal(str(trace['bonus_fraction'])) or 1 + bonus != Decimal(str(trace['damage_multiplier'])):
            raise ValueError('flying attack bonus script/projection formula disagrees')
        scaled = bonus * 10_000
        if not scaled.is_finite() or scaled != scaled.to_integral_value() or not 0 < scaled <= 65535:
            raise ValueError('flying attack bonus exceeds exact fixed-point capacity')
        result.append({'rawcode': row['unit_rawcode'], 'bonus_per_10k': int(scaled)})
    return sorted(result, key=lambda row: row['rawcode'])
