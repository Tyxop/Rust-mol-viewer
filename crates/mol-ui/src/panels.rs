use egui::Context;
use super::{UIState, RepresentationType, SelectionMode};

/// Controls panel (left side) - Representation and visual settings
pub fn controls_panel(ctx: &Context, state: &mut UIState) {
    egui::SidePanel::left("controls_panel")
        .default_width(250.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Controls");
            ui.separator();

            // Representation selection
            ui.label("Representation:");
            ui.horizontal(|ui| {
                if ui.selectable_label(state.representation == RepresentationType::VanDerWaals, "VdW").clicked() {
                    if state.representation != RepresentationType::VanDerWaals {
                        state.representation = RepresentationType::VanDerWaals;
                        state.representation_changed = true;
                    }
                }
                if ui.selectable_label(state.representation == RepresentationType::BallStick, "Ball").clicked() {
                    if state.representation != RepresentationType::BallStick {
                        state.representation = RepresentationType::BallStick;
                        state.representation_changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.selectable_label(state.representation == RepresentationType::Ribbon, "Ribbon").clicked() {
                    if state.representation != RepresentationType::Ribbon {
                        state.representation = RepresentationType::Ribbon;
                        state.representation_changed = true;
                    }
                }
                if ui.selectable_label(state.representation == RepresentationType::Surface, "Surface").clicked() {
                    if state.representation != RepresentationType::Surface {
                        state.representation = RepresentationType::Surface;
                        state.representation_changed = true;
                    }
                }
            });

            ui.add_space(10.0);

            // Current representation display
            ui.label(format!("Current: {}", state.representation));

            ui.separator();

            // Visual settings
            ui.heading("Visual Settings");

            if state.representation == RepresentationType::Surface {
                ui.label("Surface Opacity:");
                ui.add(egui::Slider::new(&mut state.alpha, 0.0..=1.0)
                    .text("Alpha")
                    .show_value(true));
            }

            ui.add_space(10.0);

            // Camera controls
            ui.heading("Camera");
            ui.label(format!("Distance: {:.1} Å", state.camera_distance));

            if ui.button("Reset Camera (R)").clicked() {
                state.reset_camera = true;
            }

            ui.add_space(10.0);

            // Quick info at bottom
            ui.separator();
            ui.small("Shortcuts:");
            ui.small("1-4: Change representation");
            ui.small("U: Toggle UI");
            ui.small("R: Reset camera");
        });
}

/// Information panel (right side) - Protein stats and performance
pub fn info_panel(ctx: &Context, state: &mut UIState) {
    egui::SidePanel::right("info_panel")
        .default_width(250.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Information");
            ui.separator();

            // Protein info
            ui.label("Protein:");
            if !state.protein_name.is_empty() {
                ui.label(format!("  {}", state.protein_name));
            } else {
                ui.label("  No protein loaded");
            }

            ui.add_space(5.0);

            egui::Grid::new("protein_stats")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Atoms:");
                    ui.label(format!("{}", format_number(state.atom_count)));
                    ui.end_row();

                    ui.label("Chains:");
                    ui.label(format!("{}", state.chain_count));
                    ui.end_row();

                    ui.label("Helices:");
                    ui.label(format!("{}", state.helix_count));
                    ui.end_row();

                    ui.label("Sheets:");
                    ui.label(format!("{}", state.sheet_count));
                    ui.end_row();
                });

            ui.separator();

            // Rendering stats
            ui.heading("Rendering");

            egui::Grid::new("render_stats")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("FPS:");
                    if state.fps > 0.0 {
                        let color = if state.fps >= 55.0 {
                            egui::Color32::GREEN
                        } else if state.fps >= 30.0 {
                            egui::Color32::YELLOW
                        } else {
                            egui::Color32::RED
                        };
                        ui.colored_label(color, format!("{:.1}", state.fps));
                    } else {
                        ui.label("--");
                    }
                    ui.end_row();

                    ui.label("Frame time:");
                    ui.label(format!("{:.2} ms", state.frame_time_ms));
                    ui.end_row();

                    ui.label("Vertices:");
                    ui.label(format!("{}", format_number(state.vertex_count)));
                    ui.end_row();

                    ui.label("Triangles:");
                    ui.label(format!("{}", format_number(state.triangle_count)));
                    ui.end_row();
                });

            // GPU Compute benchmark stats (for Van der Waals and Ball-and-Stick)
            if state.gpu_enabled && (state.representation == RepresentationType::VanDerWaals
                || state.representation == RepresentationType::BallStick) {
                ui.separator();
                ui.heading("GPU Compute");

                egui::Grid::new("gpu_stats")
                    .num_columns(2)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Status:");
                        ui.colored_label(egui::Color32::GREEN, "Enabled");
                        ui.end_row();

                        ui.label("Mode:");
                        if state.representation == RepresentationType::VanDerWaals {
                            ui.label("Full GPU");
                        } else {
                            ui.label("Hybrid (GPU atoms, CPU bonds)");
                        }
                        ui.end_row();

                        ui.label("GPU Time:");
                        ui.label(format!("{:.1} µs", state.gpu_time_us));
                        ui.end_row();

                        ui.label("CPU Time:");
                        ui.label(format!("{:.1} µs", state.cpu_time_us));
                        ui.end_row();

                        ui.label("Speedup:");
                        let color = if state.speedup >= 2.0 {
                            egui::Color32::GREEN
                        } else if state.speedup >= 1.0 {
                            egui::Color32::YELLOW
                        } else {
                            egui::Color32::RED
                        };
                        ui.colored_label(color, format!("{:.2}x", state.speedup));
                        ui.end_row();
                    });
            }

            ui.add_space(10.0);

            // Performance indicator
            if state.fps > 0.0 {
                let perf_text = if state.fps >= 55.0 {
                    "🟢 Excellent"
                } else if state.fps >= 30.0 {
                    "🟡 Good"
                } else if state.fps >= 20.0 {
                    "🟠 Fair"
                } else {
                    "🔴 Poor"
                };
                ui.label(perf_text);
            }
        });
}

/// Settings panel (floating window)
pub fn settings_panel(ctx: &Context, state: &mut UIState, open: &mut bool) {
    egui::Window::new("Settings")
        .open(open)
        .default_width(300.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Display Settings");
            ui.separator();

            ui.checkbox(&mut state.show_fps, "Show FPS counter");

            ui.add_space(5.0);

            ui.label("VSync:");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.vsync, true, "On");
                ui.selectable_value(&mut state.vsync, false, "Off");
            });

            ui.add_space(10.0);
            ui.separator();

            ui.heading("About");
            ui.label("PDB Visual - Molecular Viewer");
            ui.label("Built with Rust + wgpu + egui");
            ui.hyperlink_to("Source code", "https://github.com/yourusername/pdbvisual");
        });
}

/// Format numbers with thousands separator
fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

/// Loading panel - Shows progress when loading geometry
pub fn loading_panel(ctx: &Context, state: &UIState) {
    if !state.loading_representation {
        return;
    }

    egui::Window::new("Loading")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.heading(&state.loading_message);
                ui.add_space(10.0);
                ui.spinner();
                ui.add_space(10.0);
            });
        });
}

/// Selection panel - Shows information about selected atoms/residues
pub fn selection_panel(ctx: &Context, state: &mut UIState) {
    // Only show if there are selected atoms
    if state.selected_atoms.is_empty() {
        return;
    }

    egui::Window::new("Selection")
        .default_pos([10.0, 500.0])
        .default_width(300.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading(format!("Selected: {} atom(s)", state.selected_atoms.len()));
            ui.separator();

            // Show detailed info for the primary selected atom
            if let Some(ref info) = state.selection_info {
                egui::Grid::new("selection_info")
                    .num_columns(2)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Atom:");
                        ui.label(&info.atom_name);
                        ui.end_row();

                        ui.label("Element:");
                        ui.label(&info.element);
                        ui.end_row();

                        ui.label("Position:");
                        ui.label(format!(
                            "({:.2}, {:.2}, {:.2}) Å",
                            info.position.x, info.position.y, info.position.z
                        ));
                        ui.end_row();

                        ui.label("Residue:");
                        ui.label(format!("{} {}", info.residue_name, info.residue_seq));
                        ui.end_row();

                        ui.label("Chain:");
                        ui.label(format!("{}", info.chain_id));
                        ui.end_row();

                        ui.label("Index:");
                        ui.label(format!("{}", info.atom_index));
                        ui.end_row();
                    });

                ui.separator();
            }

            // Selection mode controls
            ui.label("Selection Mode:");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.selection_mode, SelectionMode::Single, "Single");
                ui.selectable_value(&mut state.selection_mode, SelectionMode::Multi, "Multi");
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.selection_mode, SelectionMode::Residue, "Residue");
                ui.selectable_value(&mut state.selection_mode, SelectionMode::Chain, "Chain");
            });

            ui.separator();

            // Action buttons
            if ui.button("Clear Selection (Esc)").clicked() {
                state.selected_atoms.clear();
                state.selection_info = None;
            }

            // Future features (disabled for now)
            ui.add_enabled(false, egui::Button::new("Measure Distance"));
            ui.add_enabled(false, egui::Button::new("Center on Selection"));
            ui.add_enabled(false, egui::Button::new("Hide Selected"));
        });
}

/// Animation panel - Shows animation controls for trajectories
pub fn animation_panel(ctx: &Context, state: &mut UIState) {
    // Only show if the protein is animated
    if !state.is_animated {
        return;
    }

    egui::Window::new("Animation")
        .default_pos([10.0, 100.0])
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Trajectory Animation");
            ui.separator();

            // Play/Pause/Stop buttons
            ui.horizontal(|ui| {
                let play_icon = if state.playing { "⏸" } else { "▶" };
                let play_text = if state.playing { "Pause" } else { "Play" };

                if ui.button(format!("{} {}", play_icon, play_text)).clicked() {
                    state.playing = !state.playing;
                }

                if ui.button("⏹ Stop").clicked() {
                    state.playing = false;
                    state.current_frame = 0;
                }
            });

            ui.add_space(10.0);

            // Frame slider
            ui.label(format!("Frame: {} / {}", state.current_frame + 1, state.total_frames));

            let mut frame_f32 = state.current_frame as f32;
            let max_frame = (state.total_frames - 1).max(0) as f32;

            if ui.add(egui::Slider::new(&mut frame_f32, 0.0..=max_frame)
                .show_value(false))
                .changed()
            {
                state.current_frame = frame_f32 as usize;
                // Pause when manually scrubbing
                state.playing = false;
            }

            ui.add_space(10.0);

            // Speed control (FPS)
            ui.label("Speed:");
            ui.add(egui::Slider::new(&mut state.animation_fps, 1.0..=60.0)
                .text("FPS")
                .show_value(true));

            ui.add_space(10.0);

            // Loop mode
            ui.label("Loop Mode:");
            ui.horizontal(|ui| {
                use pdb_parser::LoopMode;
                ui.selectable_value(&mut state.loop_mode, LoopMode::Once, "Once");
                ui.selectable_value(&mut state.loop_mode, LoopMode::Loop, "Loop");
                ui.selectable_value(&mut state.loop_mode, LoopMode::PingPong, "Ping-Pong");
            });

            ui.add_space(10.0);

            // Time display (if frames have time info)
            let time_ns = state.current_frame as f32 / state.animation_fps * 1000.0; // Convert to nanoseconds
            ui.separator();
            ui.label(format!("Time: {:.2} ns", time_ns));
        });
}
