import copy
import unittest
from pathlib import Path
from unittest.mock import patch
import build_native_tuning as native
import build_runtime_catalog as catalog
from project_weather import wave

class WeatherProjectionTests(unittest.TestCase):
    def test_native_interval_wave_count_mask_and_cap_derive_independently(self):
        fields={'WAVE':{'targs1':'enemies,air','cast1':'0.2','area1':'8','datab1':'12','dataf1':'25','datad1':'0.4','datac1':'2','dataa1':'3','datae1':'0','dur1':'0','herodur1':'0','buffid1':'BUFF,WBUF'}}
        profile=wave('WAVE',fields,{'WAVE':'AHbz'},native.scaled)
        self.assertEqual((profile['interval_ticks'],profile['waves'],profile['targets'],profile['maximum_damage']), (6,3,2,25))
        altered=copy.deepcopy(fields);altered['WAVE']['targs1']='friend,ground'
        with self.assertRaises(ValueError):wave('WAVE',altered,{'WAVE':'AHbz'},native.scaled)
        altered=copy.deepcopy(fields);altered['WAVE']['cast1']='0'
        with self.assertRaises(ValueError):wave('WAVE',altered,{'WAVE':'AHbz'},native.scaled)
    def test_monsoon_interval_and_duration_are_separate_from_cast_field(self):
        fields={'RAIN':{'targs1':'enemies,ground','cast1':'0.1','area1':'8','dataa1':'12','datab1':'0.5','datac1':'0','dur1':'2','herodur1':'2','buffid1':'BUFF'}}
        profile=wave('RAIN',fields,{'RAIN':'ACmo'},native.scaled)
        self.assertEqual((profile['interval_ticks'],profile['waves'],profile['building_damage_per_10k']), (15,4,0))
    def test_retained_picker_requires_ground_marker_and_uniform_choice(self):
        root=Path(__file__).resolve().parents[2];release=catalog._load_release(catalog.DEFAULT_RELEASES,'9.27','r1')
        recipes={'schema_version':1,'effects':[{'kind':'building-weather','source_kind':'unit-ability','source_key':'A0EI','unit_rawcode':'h06F'}]}
        original=catalog._retained_file_bytes
        native.build_tuning(release,root,recipes)
        for before,after in [('GetRandomInt(0,1)==0','GetRandomInt(0,2)==0'),('UNIT_TYPE_GROUND))and(unit_getAbilityLevel(COn,1098282348)','UNIT_TYPE_FLYING))and(unit_getAbilityLevel(COn,1098282348)')]:
            def changed(repo,tree,path):
                data=original(repo,tree,path)
                return data.replace(before.encode(),after.encode()) if path=='script/war3map.lua' else data
            with patch.object(catalog,'_retained_file_bytes',changed),self.assertRaises(ValueError):native.build_tuning(release,root,recipes)
