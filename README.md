<p align="center">
  <img src="data/dev.oblivius.spacecal-for-monado.svg" width="128" height="128" alt="SpaceCal for Monado">
</p>

<h1 align="center">SpaceCal for Monado</h1>

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
git clone https://github.com/99oblivius/spacecal-for-monado.git
cd spacecal-for-monado
makepkg -si -p PKGBUILD-git
```

### Fedora

```bash
sudo dnf install cargo rust gtk4-devel libadwaita-devel openxr-devel monado-devel

git clone https://github.com/99oblivius/spacecal-for-monado.git
cd spacecal-for-monado
cargo build --release --locked
sudo make PREFIX=/usr install
```

## License

MIT

## Disclaimer

Monado is a trademark of its respective owners. SpaceCal for Monado is an independent open-source project and is not affiliated with or endorsed by the Monado project.
