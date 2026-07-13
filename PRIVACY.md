# Privacy

Auto Reverse processes scroll events locally on the Mac so it can change their
direction. It does not transmit events, device identifiers, configuration, or
diagnostics over a network and contains no telemetry or analytics client.

The Debug Console keeps at most 500 recent decisions in process memory. Export
writes only when the user asks and confirms a destination in the native Save
Panel; Auto Reverse never uploads or automatically relocates that CSV.
Configuration and per-device vendor/product IDs plus an optional HID serial
number or connection-location ID are stored locally in
`~/Library/Application Support/Auto Reverse/config.toml`. The Devices tab and
tray show only a bounded serial suffix; the explicitly invoked `devices` CLI command
prints the full local identity so a rule can be diagnosed or written.

Debug Console CSV exports deliberately include vendor/product IDs and the
device's display name, but not serial numbers or location IDs. Those stronger
identifiers are used only for local rule matching and are never transmitted.

Runtime coordination files (`run.lock`, `ui.lock`, `config.toml.lock`, and the
transient `ui.activate` mailbox) stay in that same local directory. They contain
only lock state or process IDs, never scroll events, device names, or settings.

The uninstall workflow preserves this local configuration by default. User
data is removed only when `scripts/uninstall-app-bundle.sh` is invoked with
`--remove-user-data`; that option is limited to Auto Reverse's Application
Support directory and local `auto-reverse.log`.

Accessibility is required by macOS for the active event tap that observes and
modifies scroll events. It already grants event listening, so Auto Reverse does
not separately require or request Input Monitoring. Auto Reverse does not
record key presses, pointer coordinates, or application content. Its passive
AppKit gesture tap stores only whether a
gesture had at least two touching fingers and a monotonic observation time; it
does not retain touch positions, identities, pressure, gesture content, or raw
events, and none of that state is exported. A future update that adds any
network behavior must make it explicit, opt-in where appropriate, and update
this document before release.
