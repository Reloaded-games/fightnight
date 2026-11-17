"""Prepare a small verified audio bank for the game's existing one-shot events."""
import json
from native_audio import NativeAudio, ROOT


def prepare():
    converter = NativeAudio()
    catalog = json.loads((ROOT / 'dist/assets/imported/types/audio.json').read_text('utf-8'))
    slots = {
        'shot_ar': [f'SunRose_Assault_Fire_1P_Close_{i:02}' for i in range(1, 4)],
        'shot_shotgun': [f'Shotgun_VividRazor_BreakBarrel_Fire_1P_Close_{i:02}' for i in range(1, 4)],
        'ui_click': ['fort_ui_confirm_02'],
    }
    presets = []
    for slot, labels in slots.items():
        assets = []
        for label in labels:
            entry = next(e for e in catalog if e['label'] == label)
            assets.append(converter.convert(entry['id']))
            print(f'VERIFIED {slot}: {label}', flush=True)
        presets.append({'slot': slot, 'assets': assets})
    (ROOT / 'dist/assets/imported/audio-presets.json').write_text(json.dumps(presets, indent=2), encoding='utf-8')
    (ROOT / 'docs/imported/native-audio-presets.json').write_text(json.dumps(presets, indent=2), encoding='utf-8')


if __name__ == '__main__':
    prepare()
