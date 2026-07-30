//! The window.
//!
//! It holds no logic of its own: it starts the pipeline on a worker thread and
//! draws whatever that thread last reported. Everything worth testing lives in
//! the library, which is why this module has no tests and needs none.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use eframe::egui;

use panel_platform_setup as setup;
use setup::{Offer, Resolved, Stage};

enum Message {
    Resolved(Box<Resolved>),
    Progress(Stage),
    Done,
    Failed(String),
}

enum State {
    Checking,
    Confirm(Box<Resolved>),
    Working(Stage),
    Done,
    Failed(String),
}

pub struct App {
    state: State,
    rx: Receiver<Message>,
    tx: Sender<Message>,
    cancel: Arc<AtomicBool>,
    resolved: Option<Arc<Resolved>>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("App").finish_non_exhaustive()
    }
}

impl App {
    pub fn new(context: &egui::Context) -> App {
        let (tx, rx) = std::sync::mpsc::channel();

        spawn_resolve(tx.clone(), context.clone());

        App {
            state: State::Checking,
            rx,
            tx,
            cancel: Arc::new(AtomicBool::new(false)),
            resolved: None,
        }
    }

    fn drain(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            self.state = match message {
                Message::Resolved(resolved) => {
                    self.resolved = Some(Arc::new(Resolved {
                        offer: resolved.offer.clone(),
                        artefact_url: resolved.artefact_url.clone(),
                        signature_url: resolved.signature_url.clone(),
                        sums_url: resolved.sums_url.clone(),
                    }));
                    State::Confirm(resolved)
                }
                Message::Progress(stage) => State::Working(stage),
                Message::Done => State::Done,
                Message::Failed(reason) => State::Failed(reason),
            };
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();

        egui::CentralPanel::default().show(context, |ui| {
            ui.add_space(12.0);
            ui.heading("Panel Platform");
            ui.add_space(12.0);

            match &self.state {
                State::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Looking for the latest release…");
                    });
                    context.request_repaint_after(std::time::Duration::from_millis(120));
                }

                State::Confirm(resolved) => {
                    let offer = resolved.offer.clone();
                    confirm(ui, &offer);
                    ui.add_space(16.0);

                    ui.horizontal(|ui| {
                        if ui.button("Install").clicked() {
                            if let Some(resolved) = self.resolved.clone() {
                                spawn_install(
                                    resolved,
                                    self.tx.clone(),
                                    Arc::clone(&self.cancel),
                                    context.clone(),
                                );
                                self.state = State::Working(Stage::Downloading {
                                    done: 0,
                                    total: offer.bytes,
                                });
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }

                State::Working(stage) => {
                    working(ui, stage);
                    ui.add_space(16.0);
                    if ui.button("Cancel").clicked() {
                        self.cancel.store(true, Ordering::Relaxed);
                    }
                    context.request_repaint_after(std::time::Duration::from_millis(120));
                }

                State::Done => {
                    ui.label("The installer has been started.");
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("You can close this window.")
                            .color(egui::Color32::GRAY),
                    );
                    ui.add_space(16.0);
                    if ui.button("Close").clicked() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }

                State::Failed(reason) => {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), reason);
                    ui.add_space(16.0);
                    if ui.button("Close").clicked() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        });
    }
}

fn confirm(ui: &mut egui::Ui, offer: &Offer) {
    ui.label(format!("Version {} is available.", offer.version));
    ui.add_space(6.0);
    ui.label(format!(
        "This will download the {} ({}) and start it.",
        offer.kind.describe(),
        offer.size()
    ));
    ui.add_space(10.0);
    // The same thing docs/installers.md §6 says, at the moment it matters
    // rather than in a document nobody opened.
    ui.label(
        egui::RichText::new(
            "Builds are not code-signed yet. The download is checked against \
             Panel Platform's signature before anything is run.",
        )
        .small()
        .color(egui::Color32::GRAY),
    );
}

fn working(ui: &mut egui::Ui, stage: &Stage) {
    match stage {
        Stage::Checking => {
            ui.label("Looking for the latest release…");
        }
        Stage::Downloading { done, total } => {
            let fraction = if *total > 0 {
                *done as f32 / *total as f32
            } else {
                0.0
            };
            ui.label("Downloading…");
            ui.add_space(6.0);
            ui.add(egui::ProgressBar::new(fraction).show_percentage());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} of {}",
                    setup::human_size(*done),
                    setup::human_size(*total)
                ))
                .small()
                .color(egui::Color32::GRAY),
            );
        }
        Stage::Verifying => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking the download's signature…");
            });
        }
        Stage::Installing => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Starting the installer…");
            });
        }
    }
}

fn spawn_resolve(tx: Sender<Message>, context: egui::Context) {
    std::thread::spawn(move || {
        let agent = setup::net::agent();
        let message = match setup::resolve(&agent) {
            Ok(resolved) => Message::Resolved(Box::new(resolved)),
            Err(error) => Message::Failed(error.to_string()),
        };
        let _ = tx.send(message);
        context.request_repaint();
    });
}

fn spawn_install(
    resolved: Arc<Resolved>,
    tx: Sender<Message>,
    cancel: Arc<AtomicBool>,
    context: egui::Context,
) {
    std::thread::spawn(move || {
        let agent = setup::net::agent();
        let reporter = tx.clone();
        let repaint = context.clone();

        let result = setup::install(&agent, &resolved, false, &cancel, &mut |stage| {
            let _ = reporter.send(Message::Progress(stage));
            repaint.request_repaint();
        });

        let _ = tx.send(match result {
            Ok(()) => Message::Done,
            Err(error) => Message::Failed(error.to_string()),
        });
        context.request_repaint();
    });
}
