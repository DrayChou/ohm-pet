# Architecture

OHM Pet is WebView-free. It uses Rust for shared behavior and native OS windows through winit.

## Runtime

- `ohm-pet-core`: pet manifests, atlas validation and slicing, state machine, preferences.
- `ohm-pet-desktop`: native event loop, transparent always-on-top window, pointer input and event-driven rendering.
- `pixels/wgpu`: alpha-capable compositing into the native window. There is no continuous 60 FPS loop; redraws occur only when a frame or interaction changes.
- `tray-icon`: native macOS status item and Windows notification-area icon, implemented in the next integration phase.

## Resource strategy

- Idle animation advances every 480 ms.
- The event loop sleeps with `ControlFlow::WaitUntil` between deadlines.
- VSync is disabled because animations are low frequency.
- Only the current 192×208 frame is uploaded for rendering.
- Static states do not trigger redraws except for pointer or external state changes.
