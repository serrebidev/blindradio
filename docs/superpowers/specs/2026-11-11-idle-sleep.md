# Keep Windows awake during radio activity

Approved behavior: prevent automatic Windows sleep during playback or recording, while allowing the screen to turn off and the session to lock. Release the request when neither is active, and on exit. Portable and installed copies behave identically.

## Approaches

Use an application-owned Windows power request. This is scoped to radio activity and requires no administrator access or power-plan changes. Global sleep-setting changes affect other applications; away mode disables audible output. Neither fits the requested behavior.

Request SystemRequired and ExecutionRequired, never DisplayRequired or AwayModeRequired. Paused playback and paused recording do not independently require wakefulness. Background recordings do. The app sleep timer stops playback normally. Explicit sleep, hibernation, and Modern Standby battery policy remain Windows-controlled.

Windows API failures must be reported without preventing playback or repeating announcements every poll. Close the request handle on failure, inactivity, shutdown, and drop.

User resume renews active requests because Windows terminates them during explicit sleep. Automatic maintenance wakes do not renew requests, preserving Modern Standby battery policy. A station that remains selected after its stream fails is not active playback.

Validate Windows request types and lifecycle with native API tests, then run the workspace gate and dependency audit. Commit, publish through the existing Forgejo release workflow, verify all signed assets, and exercise the updater. Hardware sleep behavior on the reporting machine remains a manual check.
