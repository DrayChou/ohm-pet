# Architecture

OHM Pet is WebView-free. It uses Rust for shared behavior and native OS windows through winit.

## Runtime

- `ohm-pet-core`: pet manifests, atlas validation and slicing, state machine, preferences.
- `ohm-pet-desktop`: native event loop, transparent window, global pointer gaze, tray controls and event-driven rendering.
- macOS renderer: AppKit `NSImageView` composition with lazily cached native frames.
- Windows renderer: Win32 layered-window composition through `UpdateLayeredWindow` with premultiplied per-pixel alpha.
- `tray-icon`: native macOS status item and Windows notification-area icon, rebuilt whenever the active pet changes.

## Resource strategy

- Idle animation advances every 480 ms.
- The event loop sleeps with `ControlFlow::WaitUntil` between deadlines.
- Global pointer location is sampled every 100 ms, but redraw only occurs when the selected direction frame changes.
- Gaze remains active within 1.5 times the pet body's largest dimension.
- Static states do not trigger redraws except for pointer, autonomous or external state changes.
