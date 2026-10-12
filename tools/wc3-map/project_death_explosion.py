"""Native delayed Amnx death callback; no synthetic second destructible/helper blast."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, base):
    def number(key, scale=1):
        return int((Decimal(fields[key.lower()]) * scale).to_integral_value(rounding=ROUND_CEILING))
    if (base != 'Amnx' or set(fields['targs1'].split(',')) != {'ground','enemies','structure','debris','ward'}
            or number('levels') != 1 or any(number(k) != 0 for k in ['cost1','cool1','cast1'])
            or number('dur1',30) != number('herodur1',30)):
        raise ValueError('native death AOE requires its passive hostile ground/structure mask and common post-death delay')
    p = {'ability': int.from_bytes(recipe['source_key'].encode(),'big'), 'full_radius':number('dataa1',1024),
        'full_damage':number('datab1'), 'partial_radius':number('datac1',1024), 'partial_damage':number('datad1'),
        'delay_ticks':number('dur1',30), 'targets':5}
    if not 0 <= p['full_radius'] <= p['partial_radius'] or min(p['full_damage'],p['partial_damage']) < 0 or p['delay_ticks'] <= 0:
        raise ValueError('native death AOE requires nested nonnegative rings and clocks')
    return {**recipe,'profile':p,'provenance':{'source':'resolved/object-fields.tsv','native_base':base,
        'targets':fields['targs1'],'delay_field':'Dur1','native_damage_policy':'universal-spell-armor-type-v1'}}
