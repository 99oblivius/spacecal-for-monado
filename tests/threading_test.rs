use monado_spacecal::calibration::{CalibrationCommand, CalibrationMessage};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn test_xr_event_loop_shutdown() {
    // Create channels
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (msg_tx, msg_rx) = mpsc::channel();

    // Start the XR event loop in a background thread
    let handle = thread::spawn(move || {
        monado_spacecal::xr::xr_event_loop(cmd_rx, msg_tx);
    });

    // Give it a moment to start
    thread::sleep(Duration::from_millis(100));

    // Send shutdown command
    cmd_tx.send(CalibrationCommand::Shutdown).unwrap();

    // Wait for thread to finish (with timeout)
    let result = handle.join();
    assert!(result.is_ok(), "Thread should exit cleanly");

    // Channel should be empty (no messages expected for shutdown)
    assert!(msg_rx.try_recv().is_err(), "No messages should be received");
}

#[test]
fn test_xr_event_loop_commands_without_openxr() {
    // Create channels
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (msg_tx, msg_rx) = mpsc::channel();

    // Start the XR event loop in a background thread
    let handle = thread::spawn(move || {
        monado_spacecal::xr::xr_event_loop(cmd_rx, msg_tx);
    });

    // Give it a moment to start
    thread::sleep(Duration::from_millis(100));

    // Send a StartSampled command (should fail since OpenXR is likely not available)
    cmd_tx.send(CalibrationCommand::StartSampled {
        source_serial: "test-source".to_string(),
        target_serial: "test-target".to_string(),
        target_origin_index: 0,
        sample_count: 10,
        stage_offset: None,
    }).unwrap();

    // Wait for response (with timeout)
    let response = msg_rx.recv_timeout(Duration::from_secs(1));
    assert!(response.is_ok(), "Should receive a response");

    match response.unwrap() {
        CalibrationMessage::Error(msg) => {
            // Expected: error message about unavailability (varies by environment)
            assert!(
                msg.contains("OpenXR not available")
                    || msg.contains("not yet implemented")
                    || msg.contains("Connect to WiVRn")
                    || msg.contains("not available")
                    || msg.contains("not found"),
                "Error message should indicate unavailable or not implemented, got: {}",
                msg
            );
        }
        CalibrationMessage::Progress { .. } => {
            // If OpenXR is available, we might get progress - that's OK
        }
        CalibrationMessage::Countdown { .. } => {
            // If OpenXR is available, we might get countdown before progress - that's OK
        }
        other => panic!("Expected Error, Progress, or Countdown message, got {:?}", other),
    }

    // Send shutdown command
    cmd_tx.send(CalibrationCommand::Shutdown).unwrap();

    // Wait for thread to finish
    let result = handle.join();
    assert!(result.is_ok(), "Thread should exit cleanly");
}

#[test]
fn test_xr_event_loop_floor_calibration() {
    // Create channels
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (msg_tx, msg_rx) = mpsc::channel();

    // Start the XR event loop in a background thread
    let handle = thread::spawn(move || {
        monado_spacecal::xr::xr_event_loop(cmd_rx, msg_tx);
    });

    // Give it a moment to start
    thread::sleep(Duration::from_millis(100));

    // Send floor calibration command
    cmd_tx.send(CalibrationCommand::CalibrateFloor {
        target_serial: "test-target".to_string(),
    }).unwrap();

    // Wait for response (longer timeout since floor calibration may take time)
    let response = msg_rx.recv_timeout(Duration::from_secs(5));
    assert!(response.is_ok(), "Should receive a response");

    match response.unwrap() {
        CalibrationMessage::Error(msg) => {
            // Expected: error message about unavailability (varies by environment)
            assert!(
                msg.contains("OpenXR not available")
                    || msg.contains("not yet implemented")
                    || msg.contains("Connect to WiVRn")
                    || msg.contains("not available")
                    || msg.contains("not supported")
                    || msg.contains("not found")
                    || msg.contains("failed"),
                "Error message should indicate unavailable or failed, got: {}",
                msg
            );
        }
        CalibrationMessage::Progress { .. } => {
            // If OpenXR is available and working, we might get progress - that's OK
        }
        CalibrationMessage::FloorComplete { .. } => {
            // If floor calibration actually succeeds in test environment
        }
        other => panic!("Expected Error, Progress, or FloorComplete message, got {:?}", other),
    }

    // Send shutdown command
    cmd_tx.send(CalibrationCommand::Shutdown).unwrap();

    // Wait for thread to finish
    let result = handle.join();
    assert!(result.is_ok(), "Thread should exit cleanly");
}

#[test]
fn test_xr_event_loop_channel_closed() {
    // Create channels
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (msg_tx, _msg_rx) = mpsc::channel();

    // Start the XR event loop in a background thread
    let handle = thread::spawn(move || {
        monado_spacecal::xr::xr_event_loop(cmd_rx, msg_tx);
    });

    // Give it a moment to start
    thread::sleep(Duration::from_millis(100));

    // Drop the sender (closes the channel)
    drop(cmd_tx);

    // Wait for thread to finish (should exit when channel is closed)
    let result = handle.join();
    assert!(result.is_ok(), "Thread should exit cleanly when channel closes");
}
