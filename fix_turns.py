import sys

path = '/home/dusty/arty/src/game/loop_runner.rs'
with open(path, 'r') as f:
    c = f.read()

changes = [
    # 1. Call turn.tick() each frame before the phase match
    (
        '    // ── Phase dispatch ───────────────────────────────────────────────────────\n    use super::turn::TurnPhase;\n    match game.turn.phase {',
        '    // ── Phase dispatch ───────────────────────────────────────────────────────\n    game.turn.tick(); // counts down Acting timer and Retreating timer\n    use super::turn::TurnPhase;\n    match game.turn.phase {'
    ),
    # 2. Watching: use on_projectiles_resolved()
    (
        '                game.turn.phase = TurnPhase::Retreating { ticks_left: super::turn::RETREAT_TICKS };',
        '                game.turn.on_projectiles_resolved();'
    ),
    # 3. Retreating: remove manual countdown (turn.tick() handles it)
    (
        '        TurnPhase::Retreating { ticks_left } => {\n            apply_soldier_gravity(game);\n            process_camera_pan(cam, input);\n            if ticks_left == 0 {\n                game.turn.phase = TurnPhase::Ending;\n            } else {\n                game.turn.phase = TurnPhase::Retreating { ticks_left: ticks_left - 1 };\n            }\n        }',
        '        TurnPhase::Retreating { .. } => {\n            apply_soldier_gravity(game);\n            process_camera_pan(cam, input);\n            // turn.tick() handles the countdown and flips to Ending\n        }'
    ),
    # 4. Ending: call begin_turn() on new soldier
    (
        '            game.check_win();\n            // Camera snap to new active soldier\n            let ti = game.active_team();\n            let si = game.teams[ti].active;\n            cam.snap_to(game.teams[ti].soldiers[si].pos);',
        '            game.check_win();\n            let ti = game.active_team();\n            let si = game.teams[ti].active;\n            game.teams[ti].soldiers[si].begin_turn();\n            cam.snap_to(game.teams[ti].soldiers[si].pos);'
    ),
    # 5. fire_bazooka: use on_fired()
    (
        '    game.turn.phase = super::turn::TurnPhase::Watching;',
        '    game.turn.on_fired();'
    ),
]

for old, new in changes:
    if old in c:
        c = c.replace(old, new)
        print(f"OK: applied patch")
    else:
        print(f"MISS: could not find:\n{old[:80]}")

with open(path, 'w') as f:
    f.write(c)
print("done")
