<p align="center">
  <img src="data/dev.oblivius.monado-spacecal.svg" width="128" height="128" alt="Monado SpaceCal">
</p>

<h1 align="center">Monado SpaceCal</h1>

<p align="center">
  VR Tracking Space Calibrator for Monado/WiVRn
</p>

---

GTK4 application that aligns SLAM-tracked (WiVRn/inside-out) and lighthouse-tracked (SteamVR base stations) coordinate spaces on Linux.

## Features

- Select source and target tracking devices
- 5-second countdown for positioning
- Floor level adjustment via hand tracking
- Reset and recenter tracking origins
- Persistent device selection

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
