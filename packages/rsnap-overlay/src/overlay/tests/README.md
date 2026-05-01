# Overlay Test Layout

`tests.rs` owns shared fixtures, helper functions, and module declarations only.
New overlay behavior tests should live in the narrowest matching behavior module:

- `annotation_runtime.rs`: frozen annotation editing, text input, brush modeling, undo/redo.
- `export_actions.rs`: PNG/OCR/export authority and host-effect requests.
- `live_runtime.rs`: live-mode pointer, loupe, hotkey, and native input routing.
- `rendering_behaviors/`: visual/layout contracts split by rendering domain.
- `scroll_capture_runtime.rs`, `scroll_input_runtime.rs`, `stream_refresh_runtime.rs`: scroll capture runtime state, input freshness, and stream refresh policy.
- `self_capture_runtime.rs`: self-capture filtering, window matte capture, and deferred worker refresh.
- `toolbar_runtime.rs`: toolbar window visibility, pointer state, and drag eligibility.
- `worker_*_runtime.rs`: worker scheduling and observation contracts.

Prefer table-driven coverage when cases share the same entry point, branch shape,
and externally observable result. Keep separate tests when similar fixtures protect
different user-visible contracts, state transitions, filesystem/host effects, or
failure classifications.
