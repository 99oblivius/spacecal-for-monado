# Architecture

## Threading Model

The application runs two threads:

- **GTK main thread** — all UI code, `AppState`, and libmonado IPC calls
- **XR background thread** — OpenXR session, device pose sampling, calibration math

Both channels and the background thread are created in `build_ui()` in `src/ui/window.rs`:

```rust
let (cmd_tx, cmd_rx) = mpsc::channel();
let (msg_tx, msg_rx) = async_channel::bounded::<CalibrationMessage>(100);
thread::spawn(move || { xr_event_loop(cmd_rx, msg_tx); });
```

`cmd_tx` is wrapped in `Rc` and cloned into button callbacks. Messages from the XR thread are drained on the GTK main thread via `glib::source::idle_add_local`, which calls `msg_rx.try_recv()` in a loop during GTK idle cycles.

## Message Types

Defined in `src/calibration/mod.rs`.

**CalibrationCommand** (UI → XR thread, via `std::sync::mpsc::channel`):

| Variant | Fields | Purpose |
|---------|--------|---------|
| `StartSampled` | `source_serial`, `target_serial`, `target_origin_index`, `sample_count`, `stage_offset` | Begin pose-pair collection |
| `StartContinuous` | `source_name`, `target_name` | Start live transform updates |
| `StopContinuous` | — | Stop continuous mode |
| `CalibrateFloor` | `target_serial` | Sample floor Y from device |
| `ResetFloor` | — | Clear STAGE Y offset |
| `ResetOffset` | `category_index` | Clear a tracking origin offset |
| `Recenter` | `source_serial` | Re-align forward direction |
| `StartMovementDetection` | — | Begin velocity polling |
| `StopMovementDetection` | — | Stop velocity polling |
| `Shutdown` | — | Exit the XR thread |

**CalibrationMessage** (XR thread → UI, via `async_channel::bounded(100)`):

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Countdown` | `seconds` | Pre-calibration countdown |
| `RecenterCountdown` | `seconds` | Pre-recenter countdown |
| `Progress` | `collected`, `total` | Sample collection progress |
| `FloorProgress` | `collected`, `total` | Floor sample progress |
| `SampledComplete` | `CalibrationResult` | Calibration succeeded |
| `ContinuousUpdate` | `transform` | Real-time transform |
| `FloorComplete` | `height_adjustment` | Floor calibration done |
| `ResetComplete` | `category_index` | Origin reset done |
| `ResetFloorComplete` | — | Floor reset done |
| `RecenterComplete` | `position`, `orientation` | HMD pose after recenter |
| `MovementUpdate` | `movements: Vec<DeviceMovement>` | Per-device velocity intensities |
| `Error` | `String` | Error message for the UI |

## State Management

`SharedState` (a type alias for `Rc<RefCell<AppState>>`) lives entirely on the GTK main thread. It is the single source of truth for connection status, device lists, source/target selection, movement intensities, and battery info.

`AppState` contains a `Vec<StateListener>` (boxed `Fn(&AppState)` callbacks). Any mutation that should update the UI calls `notify_listeners()`, which iterates and fires every registered callback. Widgets register callbacks by pushing directly into `state.borrow_mut().listeners`. Selection changes are also persisted to `~/.local/share/monado-spacecal/` via `Config::save()`.

Because `SharedState` uses `Rc` (not `Arc`) it is intentionally not `Send`. All access must happen on the GTK main thread.

## Device Identification

Devices are identified by **serial number**, not by index. The `Device::unique_id()` method returns the serial if non-empty, otherwise falls back to the name. This serial is passed in `CalibrationCommand` variants and used by the XR thread to find the matching MNDX xdev by calling `d.serial() == serial || d.name() == serial`.

Devices are grouped into `Category` structs that map to Monado tracking origins. Source and target selections must come from different categories; the device dropdown filters out the other selection's category.

## Monado Integration (`src/monado.rs`)

`MonadoConnection` wraps `libmonado::Monado`. It is held inside `AppState` and only accessed from the GTK main thread.

Connection lifecycle:

- On startup, `AppState::new()` calls `monado::try_connect()` and `enumerate_devices()`.
- When disconnected, `refresh_connection()` is called by a GTK timer. It attempts reconnect only when already disconnected to avoid IPC churn.
- Battery polling is a separate lightweight call (`refresh_batteries()`) on a 5-second timer when connected; it does not re-enumerate devices.
- If the connection is lost during battery refresh, the connection is cleared and the next `refresh_connection()` poll picks it up.
- Reconnect polling is 500ms when disconnected, 5s when connected.

Key operations: `enumerate_devices()`, `apply_offset()`, `set_floor_absolute()`, `apply_recenter_absolute()`, `reset_tracking_origin()`, `refresh_batteries()`.

## XR Event Loop (`src/xr/mod.rs`)

`xr_event_loop()` runs on its own thread. It starts by attempting to create a headless OpenXR session with the following extensions:

- `MND_headless` (required)
- `MNDX_xdev_space` (optional, required for pose sampling and movement detection)
- `EXT_hand_tracking` (optional)
- `KHR_convert_timespec_time` (optional)

If the session cannot be created, the thread enters **fallback mode**: it processes commands but returns an error for any operation that needs OpenXR. The session is retried every 2 seconds, and also when 5 consecutive operation failures are detected (indicating a stale session).

Command processing:

- When movement detection is inactive, the loop blocks on `cmd_rx.recv()`.
- When movement detection is active, it uses `recv_timeout(50ms)` and polls device velocities at ~100ms intervals via MNDX space velocity queries.

**Sampled calibration**: 3-second countdown → collect pose pairs from source and target device spaces at ~30Hz → hand off to `SampleCollector` / Kabsch SVD → send `SampledComplete`.

**Floor calibration**: sample target device Y position over several frames → `FloorCalibrator` computes median → send `FloorComplete` with height delta.

**Recenter**: get HMD pose, extract yaw, compute a new STAGE offset that preserves floor Y → send `RecenterComplete` with position and orientation for the UI to apply via libmonado.

**Movement detection**: poll all MNDX xdev spaces for linear (>0.2 m/s) and angular (>0.5 rad/s) velocity. Intensity fades from 1.0 to 0.0 over 2 seconds after last movement. Sent as `MovementUpdate` to drive device-picker highlights.

## Key Source Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, creates `adw::Application` |
| `src/ui/window.rs` | `build_ui()` — window layout, channels, button handlers, message loop |
| `src/ui/state.rs` | `AppState` / `SharedState` — centralized state with listener pattern |
| `src/ui/device_list.rs` | Device dropdown widget with category grouping |
| `src/ui/device_selector.rs` | `Device` and `Category` data types |
| `src/monado.rs` | `MonadoConnection` — libmonado IPC wrapper |
| `src/xr/mod.rs` | `xr_event_loop()` — OpenXR background thread |
| `src/xr/mndx.rs` | `MNDX_xdev_space` extension wrapper |
| `src/calibration/mod.rs` | `CalibrationCommand`, `CalibrationMessage`, `CalibrationResult` |
| `src/calibration/sampled.rs` | SVD/Kabsch algorithm, `SampleCollector` |
| `src/calibration/transform.rs` | `TransformD` — double-precision transform math |
| `src/calibration/floor.rs` | `FloorCalibrator` — median Y filtering |
| `src/calibration/continuous.rs` | `ContinuousCalibrator` — sliding window (stub) |
| `src/config.rs` | JSON config persistence (`~/.local/share/monado-spacecal/`) |
| `src/preset.rs` | TOML preset system |
| `src/error.rs` | Error types (`thiserror`) |
