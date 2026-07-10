# Privacy

Auto Reverse processes scroll events locally on the Mac so it can change their
direction. It does not transmit events, device identifiers, configuration, or
diagnostics over a network and contains no telemetry or analytics client.

The Debug Console keeps at most 500 recent decisions in process memory. Export
writes only when the user asks and confirms a destination in the native Save
Panel; Auto Reverse never uploads or automatically relocates that CSV.
Configuration and per-device vendor/product IDs are stored locally in
`~/Library/Application Support/Auto Reverse/config.toml`.

Runtime coordination files (`run.lock`, `ui.lock`, `config.toml.lock`, and the
transient `ui.activate` mailbox) stay in that same local directory. They contain
only lock state or process IDs, never scroll events, device names, or settings.

Accessibility and Input Monitoring are required by macOS to observe and modify
scroll events. Auto Reverse does not record key presses, pointer coordinates, or
application content. A future update that adds any network behavior must make it
explicit, opt-in where appropriate, and update this document before release.
