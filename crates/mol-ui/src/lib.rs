use egui::{Context, FontDefinitions, Style};
use std::collections::HashSet;

pub mod panels;

pub use panels::*;

/// UI State for the molecular viewer
pub struct MolecularUI {
    pub show_controls: bool,
    pub show_info: bool,
    pub show_settings: bool,
}

impl Default for MolecularUI {
    fn default() -> Self {
        Self {
            show_controls: true,
            show_info: true,
            show_settings: false,
        }
    }
}

impl MolecularUI {
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure egui fonts and style
    pub fn configure_style(ctx: &Context) {
        let fonts = FontDefinitions::default();

        // Load default fonts with better Unicode coverage
        ctx.set_fonts(fonts);

        // Set a nice dark theme by default
        ctx.set_style(Style::default());
    }

    /// Render all UI panels
    pub fn ui(&mut self, ctx: &Context, state: &mut UIState) {
        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_controls, "Controls");
                    ui.checkbox(&mut self.show_info, "Information");
                    ui.checkbox(&mut self.show_settings, "Settings");
                    ui.separator();
                    if ui.button("Reset Camera").clicked() {
                        state.reset_camera = true;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Representation", |ui| {
                    if ui.selectable_label(state.representation == RepresentationType::VanDerWaals, "Van der Waals (1)").clicked() {
                        if state.representation != RepresentationType::VanDerWaals {
                            state.representation = RepresentationType::VanDerWaals;
                            state.representation_changed = true;
                        }
                        ui.close_menu();
                    }
                    if ui.selectable_label(state.representation == RepresentationType::BallStick, "Ball & Stick (2)").clicked() {
                        if state.representation != RepresentationType::BallStick {
                            state.representation = RepresentationType::BallStick;
                            state.representation_changed = true;
                        }
                        ui.close_menu();
                    }
                    if ui.selectable_label(state.representation == RepresentationType::Ribbon, "Ribbon (3)").clicked() {
                        if state.representation != RepresentationType::Ribbon {
                            state.representation = RepresentationType::Ribbon;
                            state.representation_changed = true;
                        }
                        ui.close_menu();
                    }
                    if ui.selectable_label(state.representation == RepresentationType::Surface, "Surface (4)").clicked() {
                        if state.representation != RepresentationType::Surface {
                            state.representation = RepresentationType::Surface;
                            state.representation_changed = true;
                        }
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    ui.label("Controls:");
                    ui.label("• Left mouse drag: Rotate");
                    ui.label("• Right mouse drag: Pan");
                    ui.label("• Mouse wheel: Zoom");
                    ui.label("• 1-4: Change representation");
                    ui.label("• U: Toggle UI");
                    ui.label("• R: Reset camera");
                    ui.label("• ESC: Quit");
                });
            });
        });

        // Control panel (left side)
        if self.show_controls {
            panels::controls_panel(ctx, state);
        }

        // Information panel (right side)
        if self.show_info {
            panels::info_panel(ctx, state);
        }

        // Settings panel (floating)
        if self.show_settings {
            panels::settings_panel(ctx, state, &mut self.show_settings);
        }

        // Selection panel (shows when atoms are selected)
        panels::selection_panel(ctx, state);

        // Animation panel (shows when trajectory is loaded)
        panels::animation_panel(ctx, state);

        // Loading panel (shows when loading geometry)
        panels::loading_panel(ctx, state);
    }
}

/// Selection mode for atom picking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Select individual atoms
    Single,
    /// Select multiple individual atoms (toggle with Ctrl+Click)
    Multi,
    /// Select entire residues
    Residue,
    /// Select entire chains
    Chain,
}

/// Information about a selected atom for display in UI
#[derive(Clone)]
pub struct AtomSelectionInfo {
    pub atom_index: usize,
    pub atom_name: String,
    pub element: String,
    pub position: glam::Vec3,
    pub residue_name: String,
    pub residue_seq: i32,
    pub chain_id: char,
}

impl AtomSelectionInfo {
    /// Create selection info from an atom
    pub fn from_atom(atom: &pdb_parser::Atom, index: usize) -> Self {
        Self {
            atom_index: index,
            atom_name: atom.name.clone(),
            element: format!("{:?}", atom.element),
            position: atom.position,
            residue_name: atom.residue.name.clone(),
            residue_seq: atom.residue.seq_num,
            chain_id: atom.residue.chain_id,
        }
    }
}

/// Shared UI state that gets passed between UI and renderer
#[derive(Clone)]
pub struct UIState {
    // Representation
    pub representation: RepresentationType,

    // Protein info
    pub protein_name: String,
    pub atom_count: usize,
    pub chain_count: usize,
    pub helix_count: usize,
    pub sheet_count: usize,

    // Rendering info
    pub fps: f32,
    pub frame_time_ms: f32,
    pub vertex_count: usize,
    pub triangle_count: usize,

    // Benchmark stats
    pub gpu_enabled: bool,
    pub gpu_time_us: f32,
    pub cpu_time_us: f32,
    pub speedup: f32,

    // Camera
    pub camera_distance: f32,
    pub reset_camera: bool,

    // Settings
    pub show_fps: bool,
    pub vsync: bool,
    pub alpha: f32,

    // Selection
    pub selected_atoms: HashSet<usize>,
    pub hovered_atom: Option<usize>,
    pub selection_mode: SelectionMode,
    pub selection_info: Option<AtomSelectionInfo>,

    // Representation loading state
    pub representation_changed: bool,
    pub loading_representation: bool,
    pub loading_message: String,
    pub pending_representation: Option<RepresentationType>,

    // Animation state
    pub is_animated: bool,
    pub playing: bool,
    pub current_frame: usize,
    pub total_frames: usize,
    pub animation_fps: f32,
    pub loop_mode: pdb_parser::LoopMode,
}

impl Default for UIState {
    fn default() -> Self {
        Self {
            representation: RepresentationType::VanDerWaals,
            protein_name: String::new(),
            atom_count: 0,
            chain_count: 0,
            helix_count: 0,
            sheet_count: 0,
            fps: 0.0,
            frame_time_ms: 0.0,
            vertex_count: 0,
            triangle_count: 0,
            gpu_enabled: false,
            gpu_time_us: 0.0,
            cpu_time_us: 0.0,
            speedup: 1.0,
            camera_distance: 100.0,
            reset_camera: false,
            show_fps: true,
            vsync: true,
            alpha: 0.7,
            selected_atoms: HashSet::new(),
            hovered_atom: None,
            selection_mode: SelectionMode::Single,
            selection_info: None,
            representation_changed: false,
            loading_representation: false,
            loading_message: String::new(),
            pending_representation: None,
            is_animated: false,
            playing: false,
            current_frame: 0,
            total_frames: 1,
            animation_fps: 30.0,
            loop_mode: pdb_parser::LoopMode::Loop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepresentationType {
    VanDerWaals,
    BallStick,
    Ribbon,
    Surface,
}

impl std::fmt::Display for RepresentationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VanDerWaals => write!(f, "Van der Waals"),
            Self::BallStick => write!(f, "Ball & Stick"),
            Self::Ribbon => write!(f, "Ribbon"),
            Self::Surface => write!(f, "Surface"),
        }
    }
}
