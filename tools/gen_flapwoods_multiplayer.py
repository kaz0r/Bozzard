#!/usr/bin/env python3
"""Derive the Steam reference scene from Flap Woods' authored artwork."""
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
source = ROOT / 'examples/demo/scenes/flap-woods.json'
scene = json.loads(source.read_text())
scene['name'] = 'Flap Woods Together'
scene.pop('game_flow', None)
objects = scene['objects']
# Native host rules own movement, collision and scores in this reference variant.
for obj in objects:
    for key in ['blueprints', 'trigger', 'collider']:
        obj.pop(key, None)
objects[:] = [o for o in objects if o['id'] not in ('bird-solid', 'score-sensor') and not o['id'].startswith('ui-')]
bird = next(o for o in objects if o['id'] == 'bird')
beak = next(o for o in objects if o['id'] == 'bird-beak')
colors = [[1., .8, .25], [.2, .8, 1.], [1., .35, .65], [.55, 1., .35]]
for slot in range(4):
    b = bird if slot == 0 else copy.deepcopy(bird)
    k = beak if slot == 0 else copy.deepcopy(beak)
    b['id'] = f'bird-{slot}'
    b['name'] = f'Player {slot + 1}'
    b['transform']['translation'] = [-5. + slot * .6, .65, slot * .05]
    b['drawable']['color'] = colors[slot]
    b['drawable']['gi_static'] = False
    k['id'] = f'bird-{slot}-beak'
    k['parent'] = b['id']
    if slot:
        objects.extend([b, k])
bird['steam_multiplayer'] = {'game': 'flap_woods', 'protocol': 2, 'app_id': 480, 'max_players': 4}
for obj in objects:
    if obj['id'].startswith('pipe-') and obj['id'].endswith(('-bottom', '-top')):
        i = int(obj['id'].split('-')[1]) - 1
        obj['transform']['translation'][1] = [-0., 1.9, -1.9][i] + (-9.05 if obj['id'].endswith('bottom') else 9.05)
    if obj['id'] in ('floor', 'ceiling'):
        obj['transform']['translation'][1] = -5.55 if obj['id'] == 'floor' else 5.55
    if obj['id'] == 'score':
        obj['text_rendering']['text'] = 'FLAP WOODS TOGETHER'
        obj['text_rendering']['font_size'] = 22.
    if obj['id'] == 'hint':
        obj['text_rendering']['text'] = 'Space: flap · L: leave lobby · Q / Esc: quit · Host starts each round'
objects.append({'id': 'steam-menu', 'name': 'Steam lobby', 'ui_canvas': {'layer': '3d', 'order': 200}})
def widget(id, text, y, kind='button', shortcuts=None):
    objects.append({'id': id, 'name': text, 'parent': 'steam-menu', 'ui_widget': {
        'kind': kind, 'text': text, 'accessible_name': text,
        'anchors': {'min': [.5, .5], 'max': [.5, .5], 'pivot': [.5, .5], 'offset': [0., y], 'size': [700. if kind == 'label' else 320., 180. if kind == 'label' else 48.]},
        'font_size': 20., 'event': id, 'shortcuts': shortcuts or [],
        'background': [.025, .045, .07, .96], 'clip_children': False,
    }})
widget('steam-countdown', '5', 0., 'label')
objects[-1]['ui_widget'].update({'visible': False, 'font_size': 96., 'padding': [24., 24., 24., 24.], 'auto_text_height': False})
objects[-1]['ui_widget']['anchors']['size'] = [112., 160.]
widget('steam-status', 'Create a lobby or accept a Steam invite.', -195., 'label')
widget('steam-create', 'Create lobby', -65., shortcuts=['C'])
widget('steam-invite', 'Invite friends', -10., shortcuts=['I'])
widget('steam-start', 'Start game (host only)', 100., shortcuts=['Enter', 'R'])
widget('steam-leave', 'Leave lobby', 155., shortcuts=['L'])
widget('steam-quit', 'Quit', 210., shortcuts=['Q'])
widget('steam-friends', 'Invite without overlay', 45.)
widget('steam-chat', 'Lobby chat', 265.)
widget('steam-chat-log', 'Lobby chat', -100., 'label')
objects[-1]['ui_widget'].update({'auto_text_height': False, 'clip_children': True, 'font_size': 18.})
objects[-1]['ui_widget']['anchors']['size'] = [700., 380.]
widget('steam-chat-draft', '> ', 150., 'label')
objects[-1]['ui_widget'].update({'auto_text_height': False, 'clip_children': True, 'font_size': 18.})
objects[-1]['ui_widget']['anchors']['size'] = [700., 112.]
widget('steam-chat-send', 'Send (Enter)', 240.)
widget('steam-chat-back', 'Back to lobby (Esc)', 295.)
for slot in range(4):
    widget(f'steam-friend-{slot}', 'Steam friend', -65. + slot * 55.)
widget('steam-friends-next', 'Next friends', 155.)
widget('steam-friends-back', 'Back to lobby', 210.)
for obj in objects:
    if obj['id'].startswith('steam-chat-') or obj['id'].startswith('steam-friend-') or obj['id'] in ('steam-friends-next', 'steam-friends-back'):
        obj['ui_widget']['visible'] = False
for obj in objects:
    obj.setdefault('transform', {'translation': [0., 0., 0.], 'rotation_degrees': [0., 0., 0.], 'scale': [1., 1., 1.]})
(ROOT / 'examples/demo/scenes/flap-woods-multiplayer.json').write_text(json.dumps(scene, indent=2) + '\n')
(ROOT / 'examples/demo/flap-woods-multiplayer.bozzard.json').write_text(json.dumps({'version': 1, 'name': 'Flap Woods Together', 'start_scene': 'scenes/flap-woods-multiplayer.json', 'view': '3d'}, indent=2) + '\n')
