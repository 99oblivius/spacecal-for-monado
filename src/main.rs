mod calibration;
mod config;
mod error;
mod monado;
mod preset;
mod ui;
mod xr;

use gtk4::prelude::*;
use libadwaita as adw;

fn main() {
    let app = adw::Application::builder()
        .application_id("dev.oblivius.monado-spacecal")
        .build();

    app.connect_activate(ui::build_ui);
    app.run();
}
