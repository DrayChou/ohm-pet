# Desktop pet implementation research

## Primary sources reviewed

- Apple RoundTransparentWindow sample: borderless transparent windows must implement content-area mouse-down and mouse-drag movement because there is no title bar.
- Shijima-Qt: uses a frameless always-on-top window, explicit dragging state, weighted/random behavior definitions, work-area boundaries, and context-dependent movement targets.
- Claude Pet: separates external task state from idle behavior, supports drag/throw, screen-edge behavior, social behavior, manual state control, and state-file integration.

## Rules adopted by OHM Pet

1. The visible pet body is the drag target. Do not hide dragging behind an invisible title strip.
2. Distinguish click from drag using window displacement between press and release.
3. Always-on-top is a user setting, not an irreversible window default.
4. Animation metadata must define valid frame counts. Transparent atlas padding is not an animation frame.
5. Autonomous behavior should be contextual and low frequency. It considers idle duration, pointer proximity, recent interactions, and the previous action.
6. Avoid repeating the same autonomous action consecutively.
7. Long inactivity produces quieter behavior and longer decision intervals.
8. External task state can override the autonomous brain later without coupling the renderer to Codex.

## Sources

- https://developer.apple.com/library/archive/samplecode/RoundTransparentWindow/Listings/Classes_CustomWindow_m.html
- https://github.com/pixelomer/Shijima-Qt
- https://github.com/xtrimsystems/claude-pet
