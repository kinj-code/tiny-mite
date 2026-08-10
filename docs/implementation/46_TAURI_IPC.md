# 46 — Tauri IPC

Use typed Tauri commands for request/response operations:

- create task;
- query task;
- list models;
- query memory;
- update settings;
- request permission.

Use Tauri channels for high-volume ordered streams:

- token streaming;
- compiler output;
- download progress;
- task progress.

Use normal events for small, low-frequency notifications.

Tauri documents commands as the typed command primitive and channels as the mechanism optimized for streaming; its general event system is not intended for low-latency/high-throughput data. citeturn0search1turn0search4

The frontend is never the authority for security decisions. Rust/core services validate permissions before privileged operations.
