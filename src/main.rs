mod calibration;
mod config;
mod error;
mod monado;
mod ui;
mod xr;

use gtk4::prelude::*;
use libadwaita as adw;

fn main() {
    let app = adw::Application::builder()
        .application_id("dev.oblivius.spacecal-for-monado")
        .build();

    app.connect_activate(ui::build_ui);
    app.run();
}
