<p align="center">
  <img src="data/dev.oblivius.monado-spacecal.svg" width="128" height="128" alt="Monado SpaceCal">
</p>

<h1 align="center">Monado SpaceCal</h1>

<p align="center">
  VR Tracking Space Calibrator for Monado/WiVRn
</p>

---

GTK4/Libadwaita application that aligns SLAM-tracked (WiVRn/inside-out) and lighthouse-tracked (SteamVR base stations) coordinate spaces using the Monado OpenXR runtime on Linux.

## Features

- Source and target device selection with categorized dropdowns grouped by tracking origin
- SVD/Kabsch sampled calibration with 3-second countdown
- Floor calibration using target device Y position (place device on floor)
- Recenter forward direction using HMD orientation
- Reset tracking origins (per-origin or all) and reset floor level
- Battery status display for all tracked devices
- Movement detection visualization (highlights moving devices in the UI)
- Persistent device selection across sessions
- Automatic Monado reconnection
- Dark theme by default

## Installation

### Arch Linux

```bash
git clone https://github.com/99oblivius/monado-spacecal.git
cd monado-spacecal
makepkg -si -p PKGBUILD-git
```

### Fedora

```bash
sudo dnf install cargo rust gtk4-devel libadwaita-devel openxr-devel monado-devel

git clone https://github.com/99oblivius/monado-spacecal.git
cd monado-spacecal
cargo build --release --locked
sudo make PREFIX=/usr install
```

## License

MIT
