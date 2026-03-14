pub mod graphics;

use eframe::egui;

pub struct AssyPlanApp {
    name: String,
}

impl Default for AssyPlanApp {
    fn default() -> Self {
        Self {
            name: "AssyPlan".to_owned(),
        }
    }
}

impl eframe::App for AssyPlanApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Hello from eframe!");
        });
    }
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("AssyPlan - Development Mode"),
        ..Default::default()
    };
    eframe::run_native(
        "AssyPlan",
        options,
        Box::new(|_cc| Box::new(AssyPlanApp::default()) as Box<dyn eframe::App>),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_app_struct_exists() {
        // Basic test to ensure the module compiles
        assert!(true);
    }
}
