mod model_manager;

use anyhow::Result;
use model_manager::ModelManager;
use mol_render::{FpsCounter, Renderer, RepresentationType};
use mol_ui::{AtomSelectionInfo, MolecularUI, UIState, RepresentationType as UIRepType};
use mol_vr::openxr;
use pdb_parser::{infer_bonds_optimized, parse_pdb_trajectory, Protein};
use std::env;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    model_manager: ModelManager,
    last_loaded_model_id: Option<usize>,
    protein_path: Option<String>,

    // VR mode flag
    vr_mode: bool,

    // ── VR molecule world transform (modified by controller input) ────────
    /// Molecule center in OpenXR stage space (meters). Default: 1.5 m ahead, 1.4 m up.
    vr_mol_position: glam::Vec3,
    /// Molecule orientation in stage space. Default: identity (Y-up).
    vr_mol_rotation: glam::Quat,
    /// Scale factor: metres per Ångström. Default 0.002 (1 Å → 2 mm, ~58 cm protein).
    vr_mol_scale: f32,

    // ── VR floating menu ─────────────────────────────────────────────────
    vr_menu_visible: bool,
    vr_prev_left_grip: bool,
    /// World-space (stage, meters) center of the floating menu quad.
    /// Updated each frame from left controller pose so the menu follows the hand.
    vr_menu_world_pos: glam::Vec3,
    /// UV coordinates (0-1) where the right controller ray hits the menu quad.
    vr_menu_pointer_uv: Option<glam::Vec2>,
    /// True for one frame when right trigger crosses 0.8 threshold.
    vr_menu_trigger_click: bool,
    vr_prev_trigger: f32,
    /// egui context used exclusively for the VR floating menu.
    vr_egui_ctx: egui::Context,
    /// egui_wgpu renderer targeting the menu texture (Rgba8UnormSrgb, no depth).
    vr_egui_renderer: Option<egui_wgpu::Renderer>,

    // ── VR controller / head tracking (updated from OpenXR each frame) ───
    vr_left_ctrl_pos: glam::Vec3,
    vr_right_ctrl_pos: glam::Vec3,
    /// Head centre (average of both eye positions) in stage space (meters).
    vr_head_pos: glam::Vec3,

    // VR input state (for edge detection)
    prev_right_grip: bool,

    // Animation state
    last_frame_time: Instant,
    frame_accumulator: f32,

    // Mouse state
    last_mouse_pos: Option<(f64, f64)>,
    mouse_pressed: bool,
    right_mouse_pressed: bool,
    mouse_down_pos: Option<(f64, f64)>,

    // Keyboard modifiers for selection
    ctrl_pressed: bool,
    shift_pressed: bool,

    // UI state
    egui_ctx: Option<egui::Context>,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    molecular_ui: MolecularUI,
    ui_state: UIState,
    fps_counter: FpsCounter,

    // Track if egui wants pointer (for event handling)
    egui_wants_pointer: bool,
}

impl App {
    fn new(protein_path: Option<String>, vr_mode: bool) -> Self {
        Self {
            window: None,
            renderer: None,
            model_manager: ModelManager::new(),
            last_loaded_model_id: None,
            protein_path,
            vr_mode,
            vr_mol_position: glam::Vec3::new(0.0, 1.4, -1.5),
            vr_mol_rotation: glam::Quat::IDENTITY,
            vr_mol_scale: 0.002,
            vr_menu_visible: false,
            vr_prev_left_grip: false,
            vr_menu_world_pos: glam::Vec3::new(0.0, 1.2, -0.5),
            vr_menu_pointer_uv: None,
            vr_menu_trigger_click: false,
            vr_prev_trigger: 0.0,
            vr_egui_ctx: egui::Context::default(),
            vr_egui_renderer: None,
            vr_left_ctrl_pos: glam::Vec3::new(-0.3, 1.0, -0.3),
            vr_right_ctrl_pos: glam::Vec3::new(0.3, 1.0, -0.3),
            vr_head_pos: glam::Vec3::new(0.0, 1.6, 0.0),
            prev_right_grip: false,
            last_frame_time: Instant::now(),
            frame_accumulator: 0.0,
            last_mouse_pos: None,
            mouse_pressed: false,
            right_mouse_pressed: false,
            mouse_down_pos: None,
            ctrl_pressed: false,
            shift_pressed: false,
            egui_ctx: None,
            egui_state: None,
            egui_renderer: None,
            molecular_ui: MolecularUI::new(),
            ui_state: UIState::default(),
            fps_counter: FpsCounter::new(),
            egui_wants_pointer: false,
        }
    }

    /// Update VR controls (joystick, teleport) and camera
    /// Note: Currently inline in event loop, kept for future refactoring
    #[allow(dead_code)]
    fn update_vr_controls(&mut self, delta_time: f32) {
        if !self.vr_mode {
            return;
        }

        if let Some(renderer) = &mut self.renderer {
            // Get VR renderer if available
            if let Some(vr_renderer) = &mut renderer.vr_renderer {
                // Sync input actions
                if let Err(e) = vr_renderer.vr_session.input.sync(&vr_renderer.vr_session.session) {
                    log::warn!("Failed to sync VR input: {}", e);
                    return;
                }

                // Get controller state
                let controller_state = match vr_renderer.vr_session.input.get_controller_state(
                    &vr_renderer.vr_session.session,
                    &vr_renderer.vr_session.stage_space,
                    openxr::Time::from_nanos(0), // TODO: Use actual predicted display time
                ) {
                    Ok(state) => state,
                    Err(e) => {
                        log::warn!("Failed to get VR controller state: {}", e);
                        return;
                    }
                };

                // Right joystick: Rotate molecule around target
                if controller_state.right_joystick.length() > 0.1 {
                    renderer.camera.apply_vr_rotation(controller_state.right_joystick, delta_time);
                }

                // Left joystick: Move camera position
                if controller_state.left_joystick.length() > 0.1 {
                    renderer.camera.apply_vr_movement(controller_state.left_joystick, delta_time);
                }

                // Teleport with left thumbstick click
                if controller_state.teleport_pressed {
                    let forward = mol_vr::VrInput::get_controller_forward(&controller_state.left_pose);
                    let teleport_distance = 50.0; // 50 Angstroms
                    renderer.camera.apply_vr_teleport(forward, teleport_distance);
                    log::info!("VR teleport triggered");
                }
            }
        }
    }

    fn load_protein_file(&mut self, path: &str) -> Result<usize> {
        log::info!("Loading PDB file: {}", path);

        let mut trajectory = parse_pdb_trajectory(path)?;

        log::info!("Successfully loaded trajectory:");
        log::info!("  {} atoms", trajectory.topology.atom_count());
        log::info!("  {} chains", trajectory.topology.chains.len());
        log::info!("  {} helices", trajectory.topology.secondary_structure.helices.len());
        log::info!("  {} sheets", trajectory.topology.secondary_structure.sheets.len());
        log::info!("  {} frames", trajectory.frame_count());

        let (min, max) = trajectory.topology.bounding_box();
        log::info!("  Bounding box: min={:?}, max={:?}", min, max);
        log::info!("  Center: {:?}", trajectory.topology.center());

        // Infer bonds for ball-and-stick rendering
        log::info!("Inferring bonds...");
        let bonds = infer_bonds_optimized(&trajectory.topology, 3.0); // 3.0 Angstrom max search radius
        trajectory.topology.bonds = bonds;

        // Center the model at origin for consistent rendering across all representations
        let center = trajectory.topology.center();
        let offset = -center;
        log::info!("Centering model at origin. Current center: {:?}, offset: {:?}", center, offset);

        // Translate all atoms
        for atom in &mut trajectory.topology.atoms {
            atom.position += offset;
        }

        // Also translate positions in all frames if animated
        for frame in &mut trajectory.frames {
            for pos in &mut frame.coords {
                *pos += offset;
            }
        }

        log::info!("Model centered. New center: {:?}", trajectory.topology.center());

        // Extract model name from file path
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let atom_count = trajectory.topology.atoms.len();

        // Add model to manager
        let model_id = self.model_manager.add_model(
            name.clone(),
            path.to_string(),
            trajectory,
        );

        // Sync with UI state - add to models list
        self.ui_state.models.push(mol_ui::ModelInfo {
            id: model_id,
            name: name.clone(),
            visible: true,
            representation: mol_ui::RepresentationType::VanDerWaals,
            atom_count,
        });

        // Update UI state (only for first model or when replacing)
        if self.model_manager.model_count() == 1 {
            let model = self.model_manager.get_model(model_id).unwrap();
            self.ui_state.protein_name = name.clone();
            self.ui_state.atom_count = model.trajectory.topology.atoms.len();
            self.ui_state.chain_count = model.trajectory.topology.chains.len();
            self.ui_state.helix_count = model.trajectory.topology.secondary_structure.helices.len();
            self.ui_state.sheet_count = model.trajectory.topology.secondary_structure.sheets.len();

            // Update animation state
            self.ui_state.is_animated = model.trajectory.is_animated();
            self.ui_state.total_frames = model.trajectory.frame_count();
            self.ui_state.current_frame = 0;
            self.ui_state.playing = false;

            if model.trajectory.is_animated() {
                log::info!("  Animation detected with {} frames", model.trajectory.frame_count());
            }
        }

        log::info!("Model loaded with ID: {}", model_id);

        Ok(model_id)
    }

    /// Open native file dialog to select a PDB file
    fn open_file_dialog(&mut self, replace_all: bool) {
        use rfd::FileDialog;

        log::info!("Opening file dialog (replace_all: {})", replace_all);

        if let Some(file) = FileDialog::new()
            .add_filter("PDB Files", &["pdb", "ent", "cif"])
            .set_title("Open PDB File")
            .pick_file()
        {
            let path = file.to_string_lossy().to_string();
            log::info!("User selected file: {}", path);
            self.handle_file_selected(path, replace_all);
        } else {
            log::info!("File dialog cancelled");
        }
    }

    /// Handle a file being selected from the dialog
    fn handle_file_selected(&mut self, path: String, replace_all: bool) {
        let was_empty = self.model_manager.is_empty();

        if replace_all {
            log::info!("Clearing all models before loading new one");
            self.model_manager.clear_all();
            self.ui_state.models.clear();
        }

        match self.load_protein_file(&path) {
            Ok(model_id) => {
                log::info!("Successfully loaded model with ID: {}", model_id);

                // Only auto-focus camera if:
                // 1. This is the first model being loaded (was_empty), OR
                // 2. We're in replace mode (replace_all)
                // In Add mode with existing models, keep the current camera view
                if was_empty || replace_all {
                    self.focus_on_model(model_id);
                } else {
                    log::info!("Model added to scene - camera position maintained");
                }
            }
            Err(e) => {
                log::error!("Failed to load PDB file: {}", e);
                // TODO: Show error message in UI
            }
        }
    }

    /// Remove a model from the scene
    fn remove_model(&mut self, id: usize) {
        if let Some(model) = self.model_manager.remove_model(id) {
            log::info!("Removed model: {} (ID: {})", model.name, model.id);

            // Remove from UI state
            self.ui_state.models.retain(|m| m.id != id);

            // If no models left, clear renderer data
            if self.model_manager.is_empty() {
                if let Some(ref mut _renderer) = self.renderer {
                    // Clear renderer data - note: for now we just log
                    // In a full implementation, we would call renderer.clear_protein_data()
                    // but this creates borrowing complexity with the current architecture
                    log::info!("All models removed");
                }
            }
        }
    }

    /// Focus camera on a specific model
    fn focus_on_model(&mut self, id: usize) {
        if let Some(model) = self.model_manager.get_model(id) {
            let protein = &model.trajectory.topology;

            // Calculate center and size
            let center = protein.center();
            let (min, max) = protein.bounding_box();
            let size = (max - min).length();

            // Position camera
            if let Some(ref mut renderer) = self.renderer {
                renderer.camera.look_at(center);
                renderer.camera.position = center + glam::Vec3::new(0.0, 0.0, size * 1.5);
                log::info!("Camera focused on model {} at center {:?}", id, center);
            }
        }
    }

    /// Frame all visible models in camera view
    fn frame_all_models(&mut self) {
        let visible_models: Vec<_> = self.model_manager.visible_models().collect();

        if visible_models.is_empty() {
            log::warn!("No visible models to frame");
            return;
        }

        // Calculate combined bounding box of all visible models
        let mut global_min = glam::Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut global_max = glam::Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);

        for model in &visible_models {
            let (min, max) = model.trajectory.topology.bounding_box();
            global_min = global_min.min(min);
            global_max = global_max.max(max);
        }

        let center = (global_min + global_max) * 0.5;
        let size = (global_max - global_min).length();

        // Position camera to view all models
        if let Some(ref mut renderer) = self.renderer {
            renderer.camera.look_at(center);
            renderer.camera.position = center + glam::Vec3::new(0.0, 0.0, size * 1.5);
            log::info!("Camera framed {} visible models. Center: {:?}, Size: {:.2}",
                visible_models.len(), center, size);
        }
    }

    /// Handle a selection click at the given screen coordinates
    fn handle_selection_click(&mut self, screen_x: f32, screen_y: f32) {
        // First, determine the clicked atom (immutable borrow scope)
        let selection_result = {
            let (renderer, protein) = match (&self.renderer, self.model_manager.visible_models().next().map(|m| &m.trajectory)) {
                (Some(r), Some(t)) => (r, &t.topology),
                _ => return,
            };

            let window_size = match &self.window {
                Some(w) => w.inner_size(),
                None => return,
            };

            // Generate ray from screen coordinates
            let ray = renderer.camera.screen_to_ray(
                screen_x,
                screen_y,
                window_size.width as f32,
                window_size.height as f32,
            );

            // Find the atom at the clicked position
            if let Some(atom_idx) = Self::pick_atom(&ray, protein, renderer) {
                log::info!(
                    "Selected atom {}: {} ({})",
                    atom_idx,
                    protein.atoms[atom_idx].name,
                    protein.atoms[atom_idx].residue.name
                );

                // Return the atom info and selection mode
                Some((
                    atom_idx,
                    mol_ui::AtomSelectionInfo::from_atom(&protein.atoms[atom_idx], atom_idx),
                    protein.clone(), // Clone protein for later use (TODO: optimize)
                ))
            } else {
                None
            }
        };

        // Now update selection (mutable borrow scope)
        if let Some((atom_idx, atom_info, protein)) = selection_result {
            let selection_mode = self.ui_state.selection_mode;
            match selection_mode {
                mol_ui::SelectionMode::Single => {
                    self.ui_state.selected_atoms.clear();
                    self.ui_state.selected_atoms.insert(atom_idx);
                }
                mol_ui::SelectionMode::Multi => {
                    // Toggle selection
                    if self.ui_state.selected_atoms.contains(&atom_idx) {
                        self.ui_state.selected_atoms.remove(&atom_idx);
                    } else {
                        self.ui_state.selected_atoms.insert(atom_idx);
                    }
                }
                mol_ui::SelectionMode::Residue => {
                    Self::select_residue_atoms(&protein, atom_idx, &mut self.ui_state.selected_atoms);
                }
                mol_ui::SelectionMode::Chain => {
                    Self::select_chain_atoms(&protein, atom_idx, &mut self.ui_state.selected_atoms);
                }
            }

            // Update selection info for UI display
            self.ui_state.selection_info = Some(atom_info);
        } else {
            // Clicked empty space - clear selection
            self.ui_state.selected_atoms.clear();
            self.ui_state.selection_info = None;
            log::info!("Selection cleared");
        }
    }

    /// Pick an atom using ray casting
    fn pick_atom(ray: &mol_core::Ray, protein: &Protein, renderer: &Renderer) -> Option<usize> {
        let octree = protein.octree.as_ref()?;

        // Query octree at multiple points along the ray
        let mut candidates = std::collections::HashSet::new();
        for i in 0..10 {
            let t = i as f32 * 10.0;
            let point = ray.point_at(t);
            let nearby = octree.query_sphere(point, 5.0); // 5 Angstrom search radius
            candidates.extend(nearby);
        }

        if candidates.is_empty() {
            return None;
        }

        // Test each candidate atom with ray-sphere intersection
        let mut closest_atom = None;
        let mut closest_dist = f32::MAX;

        for &idx in &candidates {
            let atom = &protein.atoms[idx];

            // Get radius based on current representation
            let radius = match renderer.representation {
                RepresentationType::VanDerWaals => atom.element.vdw_radius(),
                RepresentationType::BallAndStick => 0.3,
                RepresentationType::Ribbon => 0.5, // Picking on C-alpha atoms
                RepresentationType::Surface => 0.5,
                RepresentationType::SurfaceRaymarch => 0.5,
                RepresentationType::SphereRaymarch => atom.element.vdw_radius(),
            };

            // Test ray-sphere intersection
            if let Some(t) = mol_core::ray_sphere_intersection(ray, atom.position, radius) {
                if t > 0.0 && t < closest_dist {
                    closest_dist = t;
                    closest_atom = Some(idx);
                }
            }
        }

        closest_atom
    }

    /// Select all atoms in the same residue as the given atom
    fn select_residue_atoms(
        protein: &Protein,
        atom_idx: usize,
        selected_atoms: &mut std::collections::HashSet<usize>,
    ) {
        let residue_ref = &protein.atoms[atom_idx].residue;
        selected_atoms.clear();

        for (idx, atom) in protein.atoms.iter().enumerate() {
            if atom.residue.chain_id == residue_ref.chain_id
                && atom.residue.seq_num == residue_ref.seq_num
            {
                selected_atoms.insert(idx);
            }
        }

        log::info!(
            "Selected residue {}{} ({} atoms)",
            residue_ref.name,
            residue_ref.seq_num,
            selected_atoms.len()
        );
    }

    /// Select all atoms in the same chain as the given atom
    fn select_chain_atoms(
        protein: &Protein,
        atom_idx: usize,
        selected_atoms: &mut std::collections::HashSet<usize>,
    ) {
        let chain_id = protein.atoms[atom_idx].residue.chain_id;
        selected_atoms.clear();

        for (idx, atom) in protein.atoms.iter().enumerate() {
            if atom.residue.chain_id == chain_id {
                selected_atoms.insert(idx);
            }
        }

        log::info!(
            "Selected chain {} ({} atoms)",
            chain_id,
            selected_atoms.len()
        );
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title("PDB Visual - Molecular Viewer")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));

            let window = Arc::new(
                event_loop
                    .create_window(window_attributes)
                    .expect("Failed to create window"),
            );

            // Load PDB file if provided
            if let Some(path) = self.protein_path.clone() {
                match self.load_protein_file(&path) {
                    Ok(model_id) => {
                        log::info!("Initial model loaded with ID: {}", model_id);
                    }
                    Err(e) => {
                        log::error!("Failed to load PDB file: {}", e);
                    }
                }
            } else {
                log::info!("No PDB file provided.");
                log::info!("Usage: pdbvisual <pdb_file>");
            }

            // Initialize renderer (forces Vulkan backend when VR mode is active)
            let mut renderer = match pollster::block_on(Renderer::new(window.clone(), self.vr_mode)) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("═══════════════════════════════════════════");
                    log::error!("FATAL: Failed to initialize renderer");
                    log::error!("  {:#}", e);
                    log::error!("═══════════════════════════════════════════");
                    panic!("Failed to initialize renderer: {:#}", e);
                }
            };

            // Load first visible model into renderer
            if let Some(model) = self.model_manager.visible_models().next() {
                renderer.load_protein(&model.trajectory.topology);
            }

            // Initialize egui
            let egui_ctx = egui::Context::default();
            let egui_state = egui_winit::State::new(
                egui_ctx.clone(),
                egui::ViewportId::ROOT,
                &window,
                Some(window.scale_factor() as f32),
                None, // theme
                Some(2048), // max_texture_side
            );

            let egui_renderer = egui_wgpu::Renderer::new(
                &renderer.device,
                renderer.config.format,
                None,
                1,
                false,
            );

            self.egui_ctx = Some(egui_ctx);
            self.egui_state = Some(egui_state);
            self.egui_renderer = Some(egui_renderer);

            // VR-only: create a second egui renderer targeting the menu texture format
            if self.vr_mode {
                let vr_menu_renderer = egui_wgpu::Renderer::new(
                    &renderer.device,
                    wgpu::TextureFormat::Rgba8UnormSrgb, // matches vr_menu_texture
                    None,
                    1,
                    false,
                );
                self.vr_egui_renderer = Some(vr_menu_renderer);
            }

            self.renderer = Some(renderer);
            self.window = Some(window);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Let egui handle the event first and check if it consumed it
        let mut egui_consumed_event = false;
        if let (Some(egui_state), Some(window)) = (&mut self.egui_state, &self.window) {
            let response = egui_state.on_window_event(window, &event);
            egui_consumed_event = response.consumed;
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Closing application");
                event_loop.exit();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    },
                ..
            } => {
                // ESC clears selection if there is one, otherwise exits
                if !self.ui_state.selected_atoms.is_empty() {
                    self.ui_state.selected_atoms.clear();
                    self.ui_state.selection_info = None;
                    log::info!("Selection cleared");
                } else {
                    log::info!("Closing application");
                    event_loop.exit();
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(key_code),
                        ..
                    },
                ..
            } => {
                log::debug!("Key pressed: {:?}", key_code);
                if let Some(renderer) = &mut self.renderer {
                    match key_code {
                        KeyCode::Digit1 => {
                            if renderer.representation != RepresentationType::VanDerWaals {
                                renderer.representation = RepresentationType::VanDerWaals;
                                self.ui_state.representation = UIRepType::VanDerWaals;
                                self.ui_state.representation_changed = true;
                                log::info!("Switched to Van der Waals representation");
                            }
                        }
                        KeyCode::Digit2 => {
                            if renderer.representation != RepresentationType::BallAndStick {
                                renderer.representation = RepresentationType::BallAndStick;
                                self.ui_state.representation = UIRepType::BallStick;
                                self.ui_state.representation_changed = true;
                                log::info!("Switched to Ball-and-Stick representation");
                            }
                        }
                        KeyCode::Digit3 => {
                            if renderer.representation != RepresentationType::Ribbon {
                                renderer.representation = RepresentationType::Ribbon;
                                self.ui_state.representation = UIRepType::Ribbon;
                                self.ui_state.representation_changed = true;
                                log::info!("Switched to Ribbon representation");
                            }
                        }
                        KeyCode::Digit4 => {
                            if renderer.representation != RepresentationType::Surface {
                                renderer.representation = RepresentationType::Surface;
                                self.ui_state.representation = UIRepType::Surface;
                                self.ui_state.representation_changed = true;
                                log::info!("Switched to Surface representation");
                            }
                        }
                        KeyCode::Digit5 => {
                            if renderer.representation != RepresentationType::SurfaceRaymarch {
                                renderer.representation = RepresentationType::SurfaceRaymarch;
                                self.ui_state.representation = UIRepType::SurfaceRaymarch;
                                self.ui_state.representation_changed = true;
                                log::info!("Switched to Raymarching Surface representation");
                            }
                        }
                        KeyCode::Digit6 => {
                            if renderer.representation != RepresentationType::SphereRaymarch {
                                renderer.representation = RepresentationType::SphereRaymarch;
                                self.ui_state.representation = UIRepType::SphereRaymarch;
                                self.ui_state.representation_changed = true;
                                log::info!("Switched to Spheres Realtime representation");
                            }
                        }
                        KeyCode::KeyR => {
                            // Reset camera
                            if let Some(ref trajectory) = self.model_manager.visible_models().next().map(|m| &m.trajectory) {
                                let protein = &trajectory.topology;
                                renderer.camera.look_at(protein.center());
                                let (min, max) = protein.bounding_box();
                                let size = (max - min).length();
                                renderer.camera.position =
                                    protein.center() + glam::Vec3::new(0.0, 0.0, size * 1.5);
                                log::info!("Camera reset");
                            }
                        }
                        KeyCode::KeyU => {
                            // Toggle UI panels
                            self.molecular_ui.show_models = !self.molecular_ui.show_models;
                            self.molecular_ui.show_info = !self.molecular_ui.show_info;
                            log::info!("UI panels {}", if self.molecular_ui.show_info { "enabled" } else { "disabled" });
                        }
                        KeyCode::KeyA if self.ctrl_pressed => {
                            // Select all atoms (Ctrl+A)
                            if let Some(ref trajectory) = self.model_manager.visible_models().next().map(|m| &m.trajectory) {
                                let protein = &trajectory.topology;
                                self.ui_state.selected_atoms = (0..protein.atoms.len()).collect();
                                log::info!("Selected all {} atoms", protein.atoms.len());

                                // Set selection info to first atom
                                if !protein.atoms.is_empty() {
                                    self.ui_state.selection_info = Some(
                                        AtomSelectionInfo::from_atom(&protein.atoms[0], 0)
                                    );
                                }
                            }
                        }
                        // Track modifier keys for selection
                        KeyCode::ControlLeft | KeyCode::ControlRight => {
                            self.ctrl_pressed = true;
                        }
                        KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                            self.shift_pressed = true;
                        }
                        _ => {}
                    }
                }
            }

            // Handle key releases for modifiers
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Released,
                        physical_key: PhysicalKey::Code(key_code),
                        ..
                    },
                ..
            } => {
                match key_code {
                    KeyCode::ControlLeft | KeyCode::ControlRight => {
                        self.ctrl_pressed = false;
                    }
                    KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                        self.shift_pressed = false;
                    }
                    _ => {}
                }
            }

            WindowEvent::Resized(physical_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(physical_size);
                }
            }

            WindowEvent::RedrawRequested => {
                // Update FPS counter
                self.fps_counter.tick();

                // Handle file dialog triggers BEFORE borrowing renderer
                if self.ui_state.trigger_load_add {
                    self.ui_state.trigger_load_add = false;
                    self.open_file_dialog(false); // Add to scene
                }

                if self.ui_state.trigger_load_replace {
                    self.ui_state.trigger_load_replace = false;
                    self.open_file_dialog(true); // Replace all
                }

                // Handle model removal
                if let Some(model_id) = self.ui_state.model_to_remove.take() {
                    self.remove_model(model_id);
                }

                // Handle frame all visible models
                if self.ui_state.trigger_frame_all {
                    self.ui_state.trigger_frame_all = false;
                    self.frame_all_models();
                }

                // Sync model visibility and representation changes from UI to ModelManager
                for ui_model in &self.ui_state.models.clone() {
                    if let Some(model) = self.model_manager.get_model_mut(ui_model.id) {
                        model.visible = ui_model.visible;
                        model.representation = match ui_model.representation {
                            UIRepType::VanDerWaals => RepresentationType::VanDerWaals,
                            UIRepType::BallStick => RepresentationType::BallAndStick,
                            UIRepType::Ribbon => RepresentationType::Ribbon,
                            UIRepType::Surface => RepresentationType::Surface,
                            UIRepType::SurfaceRaymarch => RepresentationType::SurfaceRaymarch,
                            UIRepType::SphereRaymarch => RepresentationType::SphereRaymarch,
                        };
                    }
                }

                if let (Some(renderer), Some(egui_ctx), Some(egui_state), Some(egui_renderer), Some(window)) =
                    (&mut self.renderer, &self.egui_ctx, &mut self.egui_state, &mut self.egui_renderer, &self.window)
                {
                    // Update UI state with current renderer state
                    self.ui_state.fps = self.fps_counter.fps();
                    self.ui_state.frame_time_ms = self.fps_counter.frame_time_ms();
                    self.ui_state.camera_distance = renderer.camera.position.distance(renderer.camera.target);

                    // GPU Compute benchmark stats
                    let bench_stats = renderer.get_benchmark_stats();
                    self.ui_state.gpu_enabled = bench_stats.gpu_enabled;
                    self.ui_state.gpu_time_us = bench_stats.gpu_compute_time_us;
                    self.ui_state.cpu_time_us = bench_stats.cpu_compute_time_us;
                    self.ui_state.speedup = bench_stats.speedup_factor();

                    // Store old representation to detect changes from UI
                    let old_representation = self.ui_state.representation;

                    // Build egui UI
                    let raw_input = egui_state.take_egui_input(window);
                    let full_output = egui_ctx.run(raw_input, |ctx| {
                        // Use comprehensive molecular UI
                        self.molecular_ui.ui(ctx, &mut self.ui_state);
                    });

                    // Update whether egui wants pointer input
                    self.egui_wants_pointer = egui_ctx.wants_pointer_input();

                    // Handle platform output
                    egui_state.handle_platform_output(window, full_output.platform_output);

                    // Sync UI state changes back to renderer
                    if self.ui_state.representation != old_representation {
                        // Representation changed from UI or keyboard
                        renderer.representation = match self.ui_state.representation {
                            UIRepType::VanDerWaals => RepresentationType::VanDerWaals,
                            UIRepType::BallStick => RepresentationType::BallAndStick,
                            UIRepType::Ribbon => RepresentationType::Ribbon,
                            UIRepType::Surface => RepresentationType::Surface,
                            UIRepType::SurfaceRaymarch => RepresentationType::SurfaceRaymarch,
                            UIRepType::SphereRaymarch => RepresentationType::SphereRaymarch,
                        };

                        // Also update all model representations to match global
                        for model_info in &mut self.ui_state.models {
                            model_info.representation = self.ui_state.representation;
                        }

                        // Also update model_manager models to ensure sync in same frame
                        for model in self.model_manager.all_models_mut() {
                            model.representation = renderer.representation;
                        }

                        self.ui_state.representation_changed = true;
                        log::info!("Representation changed to {:?}", renderer.representation);
                    }

                    // Handle representation changes - TWO FRAME SYSTEM for loading UI
                    // Frame 1: Detect change, show loading UI
                    // Frame 2: Generate geometry (in next frame so UI renders)

                    if self.ui_state.representation_changed && !self.model_manager.is_empty() {
                        // First frame: Just mark as pending and show loading UI
                        self.ui_state.pending_representation = Some(self.ui_state.representation);
                        self.ui_state.representation_changed = false;

                        // Set loading message based on representation
                        self.ui_state.loading_message = match self.ui_state.representation {
                            UIRepType::BallStick => "Loading Ball & Stick...".to_string(),
                            UIRepType::Ribbon => "Loading Ribbon...".to_string(),
                            UIRepType::Surface => "Generating Surface (this may take a moment)...".to_string(),
                            UIRepType::SurfaceRaymarch => "Preparing Raymarching Surface...".to_string(),
                            UIRepType::SphereRaymarch => "Preparing Sphere Raymarch...".to_string(),
                            UIRepType::VanDerWaals => String::new(),
                        };

                        self.ui_state.loading_representation = match self.ui_state.representation {
                            UIRepType::VanDerWaals | UIRepType::SurfaceRaymarch | UIRepType::SphereRaymarch => false,
                            _ => true,
                        };
                    }

                    // Second frame: Actually generate the geometry
                    if let Some(pending_rep) = self.ui_state.pending_representation {
                        if let Some(ref trajectory) = self.model_manager.visible_models().next().map(|m| &m.trajectory) {
                            let protein = &trajectory.topology;
                            match pending_rep {
                                UIRepType::BallStick => {
                                    log::info!("Generating ball-stick geometry...");
                                    renderer.ensure_ball_stick(protein);
                                }
                                UIRepType::Ribbon => {
                                    log::info!("Generating ribbon geometry...");
                                    renderer.ensure_ribbon(protein);
                                }
                                UIRepType::Surface => {
                                    log::info!("Generating surface geometry...");
                                    renderer.ensure_surface(protein);
                                }
                                UIRepType::SurfaceRaymarch => {
                                    log::info!("Preparing raymarching surface...");
                                    renderer.ensure_surface_raymarch(protein);
                                }
                                UIRepType::SphereRaymarch => {
                                    log::info!("Preparing sphere raymarch...");
                                    renderer.ensure_sphere_raymarch(protein);
                                }
                                UIRepType::VanDerWaals => {
                                    // VdW is always loaded, nothing to do
                                }
                            }
                        }

                        // Clear pending and loading state
                        self.ui_state.pending_representation = None;
                        self.ui_state.loading_representation = false;
                    }

                    // Handle camera reset from UI
                    if self.ui_state.reset_camera {
                        if let Some(ref trajectory) = self.model_manager.visible_models().next().map(|m| &m.trajectory) {
                            let protein = &trajectory.topology;
                            renderer.camera.look_at(protein.center());
                            let (min, max) = protein.bounding_box();
                            let size = (max - min).length();
                            renderer.camera.position = protein.center() + glam::Vec3::new(0.0, 0.0, size * 1.5);
                            log::info!("Camera reset from UI");
                        }
                        self.ui_state.reset_camera = false;
                    }

                    // Update surface alpha from UI (only if surface renderer is loaded)
                    if let Some(ref mut surface_renderer) = renderer.surface_renderer {
                        surface_renderer.set_alpha(&renderer.queue, self.ui_state.alpha);
                    }

                    // We'll process and render each visible model in the render loop below

                    // Handle animation frame updates
                    if self.ui_state.is_animated && self.ui_state.playing {
                        let now = Instant::now();
                        let delta_time = now.duration_since(self.last_frame_time).as_secs_f32();
                        self.last_frame_time = now;
                        self.frame_accumulator += delta_time;

                        let frame_duration = 1.0 / self.ui_state.animation_fps;

                        // Advance frames
                        while self.frame_accumulator >= frame_duration {
                            self.frame_accumulator -= frame_duration;

                            use pdb_parser::LoopMode;
                            match self.ui_state.loop_mode {
                                LoopMode::Once => {
                                    if self.ui_state.current_frame + 1 < self.ui_state.total_frames {
                                        self.ui_state.current_frame += 1;
                                    } else {
                                        self.ui_state.playing = false;
                                    }
                                }
                                LoopMode::Loop => {
                                    self.ui_state.current_frame = (self.ui_state.current_frame + 1) % self.ui_state.total_frames;
                                }
                                LoopMode::PingPong => {
                                    // TODO: Implement ping-pong (requires direction tracking)
                                    self.ui_state.current_frame = (self.ui_state.current_frame + 1) % self.ui_state.total_frames;
                                }
                            }
                        }

                        // Update atom positions from current frame
                        if let Some(ref trajectory) = self.model_manager.visible_models().next().map(|m| &m.trajectory) {
                            if let Some(frame) = trajectory.frames.get(self.ui_state.current_frame) {
                                renderer.update_atom_positions(&trajectory.topology, &frame.coords);
                            }
                        }
                    }

                    // ── VR Controller Input ──────────────────────────────────────────────
                    // Joysticks rotate/scale the molecule transform (mol_to_world).
                    // Left grip toggles the floating menu.
                    // Right grip picks atoms.
                    let mut vr_grip_selection: Option<mol_vr::Pose> = None;

                    if self.vr_mode {
                        let dt = (self.fps_counter.frame_time_ms() / 1000.0).clamp(0.001, 0.1);

                        if let Some(vr_renderer) = &mut renderer.vr_renderer {
                            if let Ok(()) = vr_renderer.vr_session.input.sync(&vr_renderer.vr_session.session) {
                                if let Ok(cs) = vr_renderer.vr_session.input.get_controller_state(
                                    &vr_renderer.vr_session.session,
                                    &vr_renderer.vr_session.stage_space,
                                    openxr::Time::from_nanos(0),
                                ) {
                                    // Right joystick → rotate molecule (Y axis = yaw, X axis = pitch)
                                    if cs.right_joystick.length() > 0.1 {
                                        let yaw   = glam::Quat::from_rotation_y(-cs.right_joystick.x * 2.0 * dt);
                                        let pitch = glam::Quat::from_rotation_x( cs.right_joystick.y * 2.0 * dt);
                                        self.vr_mol_rotation = yaw * pitch * self.vr_mol_rotation;
                                    }

                                    // Left joystick Y → zoom (scale)
                                    if cs.left_joystick.y.abs() > 0.1 {
                                        let factor = 1.0 + cs.left_joystick.y * 1.5 * dt;
                                        self.vr_mol_scale = (self.vr_mol_scale * factor).clamp(0.0003, 0.05);
                                    }

                                    // Left joystick X → slide molecule left/right
                                    if cs.left_joystick.x.abs() > 0.1 {
                                        self.vr_mol_position.x += cs.left_joystick.x * 0.5 * dt;
                                    }

                                    // Store controller poses for sphere rendering & menu placement
                                    self.vr_left_ctrl_pos  = cs.left_pose.position;
                                    self.vr_right_ctrl_pos = cs.right_pose.position;
                                    // Menu appears at left controller position (updated below from views for billboarding)
                                    self.vr_menu_world_pos = cs.left_pose.position;

                                    // Left grip (edge) → toggle floating menu
                                    let left_grip = cs.left_grip_pressed;
                                    if left_grip && !self.vr_prev_left_grip {
                                        self.vr_menu_visible = !self.vr_menu_visible;
                                        log::info!("VR menu: {}", if self.vr_menu_visible { "opened" } else { "closed" });
                                    }
                                    self.vr_prev_left_grip = left_grip;

                                    // Right trigger (edge at 0.8) → click on menu or nothing
                                    let trig = cs.right_trigger_value;
                                    self.vr_menu_trigger_click = trig > 0.8 && self.vr_prev_trigger <= 0.8;
                                    self.vr_prev_trigger = trig;

                                    // Compute controller ray → menu UV intersection (right controller)
                                    if self.vr_menu_visible {
                                        use glam::Vec3;
                                        let ray_origin = cs.right_pose.position;
                                        let ray_dir = mol_vr::VrInput::get_controller_forward(&cs.right_pose);
                                        // Menu quad follows left controller; normal faces roughly toward origin (+Z in stage)
                                        let menu_center = self.vr_menu_world_pos;
                                        let menu_normal = Vec3::new(0.0, 0.0, 1.0);
                                        let denom = menu_normal.dot(ray_dir);
                                        if denom.abs() > 0.001 {
                                            let t = menu_normal.dot(menu_center - ray_origin) / denom;
                                            if t > 0.0 {
                                                let hit = ray_origin + ray_dir * t;
                                                let local = hit - menu_center;
                                                let u = (local.x / 0.6 + 0.5).clamp(0.0, 1.0);
                                                let v = (0.5 - local.y / 0.3).clamp(0.0, 1.0);
                                                self.vr_menu_pointer_uv = Some(glam::Vec2::new(u, v));
                                            } else {
                                                self.vr_menu_pointer_uv = None;
                                            }
                                        } else {
                                            self.vr_menu_pointer_uv = None;
                                        }
                                    } else {
                                        self.vr_menu_pointer_uv = None;
                                    }

                                    // Right grip (edge) → pick atom when menu is closed
                                    if cs.right_grip_pressed && !self.prev_right_grip && !self.vr_menu_visible {
                                        vr_grip_selection = Some(cs.right_pose);
                                    }
                                    self.prev_right_grip = cs.right_grip_pressed;
                                }
                            }
                        }
                    }

                    // Handle VR grip selection (outside renderer borrow)
                    if let Some(grip_pose) = vr_grip_selection {
                        if let Some(ref trajectory) = self.model_manager.visible_models().next().map(|m| &m.trajectory) {
                            let protein = &trajectory.topology;
                            let ray = mol_vr::controller_ray(&grip_pose);

                            if let Some(atom_idx) = Self::pick_atom(&ray, protein, renderer) {
                                log::info!("VR atom selected: {} ({:?})", atom_idx, protein.atoms[atom_idx].element);

                                // Handle multi-selection (same logic as desktop mouse click)
                                if self.shift_pressed {
                                    // Shift: Add to selection
                                    self.ui_state.selected_atoms.insert(atom_idx);
                                } else if self.ctrl_pressed {
                                    // Ctrl: Toggle
                                    if !self.ui_state.selected_atoms.insert(atom_idx) {
                                        self.ui_state.selected_atoms.remove(&atom_idx);
                                    }
                                } else {
                                    // Normal: Replace selection
                                    self.ui_state.selected_atoms.clear();
                                    self.ui_state.selected_atoms.insert(atom_idx);
                                }

                                // Update selection info for UI
                                self.ui_state.selection_info =
                                    Some(mol_ui::AtomSelectionInfo::from_atom(&protein.atoms[atom_idx], atom_idx));
                            } else {
                                // No atom hit - clear selection (if not shift/ctrl)
                                if !self.shift_pressed && !self.ctrl_pressed {
                                    self.ui_state.selected_atoms.clear();
                                    self.ui_state.selection_info = None;
                                    log::info!("VR selection cleared");
                                }
                            }
                        }
                    }

                    // ── VR Frame Loop ────────────────────────────────────────────────────
                    // MUST call poll_events + begin_frame + end_frame every frame.
                    // poll_events drives the session state machine (IDLE→READY→FOCUSED).
                    // Without this the Quest 2 shows only the loading screen forever.
                    if self.vr_mode {
                        if let Some(ref mut vr) = renderer.vr_renderer {
                            // Handle session state transitions
                            match vr.poll_events() {
                                Ok(false) => { event_loop.exit(); }
                                Ok(true) => {}
                                Err(e) => log::warn!("VR poll_events: {}", e),
                            }

                            use mol_vr::SessionState;
                            // Include Ready: the frame loop MUST start right after session.begin()
                            // so the runtime can transition Ready → Synchronized → Focused.
                            // Without this the runtime never advances past Ready.
                            let session_active = matches!(
                                vr.vr_session.session_state,
                                SessionState::Ready
                                    | SessionState::Synchronized
                                    | SessionState::Visible
                                    | SessionState::Focused
                            );

                            if session_active {
                                match vr.begin_frame() {
                                    Ok(frame_state) => {
                                        if frame_state.should_render {
                                            // Build per-eye camera uniforms directly from OpenXR 6DoF poses.
                                            // Scale: 1 Å = 2 mm (0.002 m), molecule placed 1.5 m ahead at 1.4 m height.
                                            // This properly tracks head rotation and translation.
                                            // ── mol_to_world from controller-driven state ──────
                                            // Hoisted here so it's also available for the menu transform below.
                                            let mol_to_world = glam::Mat4::from_scale_rotation_translation(
                                                glam::Vec3::splat(self.vr_mol_scale),
                                                self.vr_mol_rotation,
                                                self.vr_mol_position,
                                            );
                                            if let Ok(views) = vr.vr_session.get_view_configs() {
                                                // Track head position for menu billboarding
                                                self.vr_head_pos = (views[0].position + views[1].position) * 0.5;

                                                let left_uniform = mol_render::vr_renderer::VrRenderer::build_eye_uniform(
                                                    &views[0], 0.02, 100.0, mol_to_world,
                                                );
                                                let right_uniform = mol_render::vr_renderer::VrRenderer::build_eye_uniform(
                                                    &views[1], 0.02, 100.0, mol_to_world,
                                                );
                                                renderer.queue.write_buffer(&vr.left_camera_buffer, 0, bytemuck::cast_slice(&[left_uniform]));
                                                renderer.queue.write_buffer(&vr.right_camera_buffer, 0, bytemuck::cast_slice(&[right_uniform]));
                                            }

                                            // ── Controller sphere transforms ──────────────────
                                            // Sphere radius = 3 cm in stage space.
                                            // Pre-multiply by mol_to_world.inverse() so the shader's
                                            // view_proj (which includes mol_to_world) cancels out.
                                            {
                                                let r = 0.03f32; // 3 cm radius
                                                let left_stage  = glam::Mat4::from_scale_rotation_translation(glam::Vec3::splat(r), glam::Quat::IDENTITY, self.vr_left_ctrl_pos);
                                                let right_stage = glam::Mat4::from_scale_rotation_translation(glam::Vec3::splat(r), glam::Quat::IDENTITY, self.vr_right_ctrl_pos);
                                                vr.update_controller_transforms(
                                                    &renderer.queue,
                                                    mol_to_world.inverse() * left_stage,
                                                    mol_to_world.inverse() * right_stage,
                                                );
                                            }

                                            // ── Render egui menu to texture (before eye passes) ─
                                            if self.vr_menu_visible {
                                                if let Some(ref mut menu_rdr) = self.vr_egui_renderer {
                                                    let px = 2.0f32; // pixels_per_point: 2× → crisper
                                                    let screen_w = 512u32;
                                                    let screen_h = 256u32;

                                                    // Build egui input; inject controller ray as pointer
                                                    let mut raw = egui::RawInput {
                                                        screen_rect: Some(egui::Rect::from_min_size(
                                                            egui::Pos2::ZERO,
                                                            egui::Vec2::new(screen_w as f32 / px, screen_h as f32 / px),
                                                        )),
                                                        ..Default::default()
                                                    };
                                                    self.vr_egui_ctx.set_pixels_per_point(px);
                                                    if let Some(uv) = self.vr_menu_pointer_uv {
                                                        let ppos = egui::Pos2::new(
                                                            uv.x * screen_w as f32 / px,
                                                            uv.y * screen_h as f32 / px,
                                                        );
                                                        raw.events.push(egui::Event::PointerMoved(ppos));
                                                        if self.vr_menu_trigger_click {
                                                            raw.events.push(egui::Event::PointerButton { pos: ppos, button: egui::PointerButton::Primary, pressed: true,  modifiers: Default::default() });
                                                            raw.events.push(egui::Event::PointerButton { pos: ppos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
                                                        }
                                                    }

                                                    // Menu state captures
                                                    let mut repr_choice: Option<RepresentationType> = None;
                                                    let mut do_reset = false;
                                                    let mut do_scale_up   = false;
                                                    let mut do_scale_down = false;
                                                    let cur_repr = renderer.representation;
                                                    let cur_scale = self.vr_mol_scale;

                                                    let full_out = self.vr_egui_ctx.run(raw, |ctx| {
                                                        ctx.set_visuals(egui::Visuals::dark());
                                                        egui::CentralPanel::default().show(ctx, |ui| {
                                                            ui.heading("PDB Visual VR");
                                                            ui.separator();
                                                            ui.label("Representation:");
                                                            ui.horizontal(|ui| {
                                                                let opts = [
                                                                    ("VdW",       RepresentationType::VanDerWaals),
                                                                    ("Ball+Stick",RepresentationType::BallAndStick),
                                                                    ("Ribbon",    RepresentationType::Ribbon),
                                                                    ("Surface",   RepresentationType::Surface),
                                                                ];
                                                                for (label, rt) in &opts {
                                                                    if ui.selectable_label(cur_repr == *rt, *label).clicked() {
                                                                        repr_choice = Some(*rt);
                                                                    }
                                                                }
                                                            });
                                                            ui.separator();
                                                            ui.horizontal(|ui| {
                                                                if ui.button("Reset").clicked()   { do_reset = true; }
                                                                if ui.button("Scale+").clicked()  { do_scale_up = true; }
                                                                if ui.button("Scale-").clicked()  { do_scale_down = true; }
                                                            });
                                                            ui.label(format!("Scale: {:.1}%", cur_scale / 0.002 * 100.0));
                                                            ui.separator();
                                                            ui.small("R.Stick: Rotate  L.Stick: Zoom/Move");
                                                            ui.small("L.Grip: Menu    R.Grip: Pick atom");
                                                        });
                                                    });

                                                    // Apply button actions
                                                    if let Some(rt) = repr_choice {
                                                        self.ui_state.representation = match rt {
                                                            RepresentationType::VanDerWaals => UIRepType::VanDerWaals,
                                                            RepresentationType::BallAndStick => UIRepType::BallStick,
                                                            RepresentationType::Ribbon => UIRepType::Ribbon,
                                                            RepresentationType::Surface => UIRepType::Surface,
                                                            _ => UIRepType::VanDerWaals,
                                                        };
                                                    }
                                                    if do_reset {
                                                        self.vr_mol_position = glam::Vec3::new(0.0, 1.4, -1.5);
                                                        self.vr_mol_rotation = glam::Quat::IDENTITY;
                                                        self.vr_mol_scale    = 0.002;
                                                    }
                                                    if do_scale_up   { self.vr_mol_scale = (self.vr_mol_scale * 1.5).min(0.05); }
                                                    if do_scale_down { self.vr_mol_scale = (self.vr_mol_scale / 1.5).max(0.0003); }

                                                    // Update egui textures
                                                    for (id, delta) in &full_out.textures_delta.set {
                                                        menu_rdr.update_texture(&renderer.device, &renderer.queue, *id, delta);
                                                    }
                                                    let prims = self.vr_egui_ctx.tessellate(full_out.shapes, px);
                                                    let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [screen_w, screen_h], pixels_per_point: px };

                                                    let mut menu_enc = renderer.device.create_command_encoder(
                                                        &wgpu::CommandEncoderDescriptor { label: Some("VR Menu Enc") }
                                                    );
                                                    menu_rdr.update_buffers(&renderer.device, &renderer.queue, &mut menu_enc, &prims, &sd);
                                                    {
                                                        let mut rpass = menu_enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                                                            label: Some("VR Menu Pass"),
                                                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                                                view: &vr.vr_menu_texture_view,
                                                                resolve_target: None,
                                                                ops: wgpu::Operations {
                                                                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.08, g: 0.08, b: 0.12, a: 0.95 }),
                                                                    store: wgpu::StoreOp::Store,
                                                                },
                                                            })],
                                                            depth_stencil_attachment: None,
                                                            timestamp_writes: None,
                                                            occlusion_query_set: None,
                                                        }).forget_lifetime();
                                                        menu_rdr.render(&mut rpass, &prims, &sd);
                                                    }
                                                    renderer.queue.submit(std::iter::once(menu_enc.finish()));

                                                    // ── Billboard menu toward user's head ────────
                                                    // Compute rotation so the menu quad always faces the user.
                                                    let menu_pos = self.vr_menu_world_pos;
                                                    let to_head  = (self.vr_head_pos - menu_pos).normalize();
                                                    // Menu local +Z should point toward the head.
                                                    let menu_rot = if to_head.length_squared() > 0.0001 {
                                                        glam::Quat::from_rotation_arc(glam::Vec3::Z, to_head)
                                                    } else {
                                                        glam::Quat::IDENTITY
                                                    };
                                                    // 60 cm × 30 cm panel in stage space.
                                                    // Pre-multiply by mol_to_world.inverse() so the shader's
                                                    // view_proj (= proj * eye_from_world * mol_to_world) cancels.
                                                    let menu_stage = glam::Mat4::from_scale_rotation_translation(
                                                        glam::Vec3::new(0.6, 0.3, 1.0),
                                                        menu_rot,
                                                        menu_pos,
                                                    );
                                                    vr.update_menu_transform(&renderer.queue, mol_to_world.inverse() * menu_stage);

                                                    for id in &full_out.textures_delta.free {
                                                        menu_rdr.free_texture(id);
                                                    }
                                                }
                                            }

                                            // Acquire swapchain images
                                            let left_idx = vr.vr_session.left_swapchain.as_mut()
                                                .and_then(|s| s.acquire_image().ok());
                                            let right_idx = vr.vr_session.right_swapchain.as_mut()
                                                .and_then(|s| s.acquire_image().ok());

                                            if let (Some(left_idx), Some(right_idx)) = (left_idx, right_idx) {
                                                let mut vr_enc = renderer.device.create_command_encoder(
                                                    &wgpu::CommandEncoderDescriptor { label: Some("VR Encoder") }
                                                );

                                                let menu_vis = self.vr_menu_visible;

                                                // ── LEFT EYE ──────────────────────────────────────
                                                {
                                                    let color_view = &vr.vr_session.left_swapchain.as_ref().unwrap().views[left_idx];
                                                    let mut pass = vr_enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                                                        label: Some("VR Left Eye"),
                                                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                                            view: color_view,
                                                            resolve_target: None,
                                                            ops: wgpu::Operations {
                                                                load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.02, g: 0.02, b: 0.05, a: 1.0 }),
                                                                store: wgpu::StoreOp::Store,
                                                            },
                                                        })],
                                                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                                            view: &vr.left_depth_view,
                                                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                                                            stencil_ops: None,
                                                        }),
                                                        timestamp_writes: None,
                                                        occlusion_query_set: None,
                                                    });
                                                    pass.set_bind_group(0, &vr.left_camera_bind_group, &[]);
                                                    if let Some(ref vr_sph) = renderer.vr_spheres_renderer {
                                                        vr_sph.render(&mut pass);
                                                    }
                                                    vr.render_controllers(&mut pass, &vr.left_camera_bind_group);
                                                    if menu_vis { vr.render_menu_quad(&mut pass, &vr.left_camera_bind_group); }
                                                }

                                                // ── RIGHT EYE ─────────────────────────────────────
                                                {
                                                    let color_view = &vr.vr_session.right_swapchain.as_ref().unwrap().views[right_idx];
                                                    let mut pass = vr_enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                                                        label: Some("VR Right Eye"),
                                                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                                            view: color_view,
                                                            resolve_target: None,
                                                            ops: wgpu::Operations {
                                                                load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.02, g: 0.02, b: 0.05, a: 1.0 }),
                                                                store: wgpu::StoreOp::Store,
                                                            },
                                                        })],
                                                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                                            view: &vr.right_depth_view,
                                                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                                                            stencil_ops: None,
                                                        }),
                                                        timestamp_writes: None,
                                                        occlusion_query_set: None,
                                                    });
                                                    pass.set_bind_group(0, &vr.right_camera_bind_group, &[]);
                                                    if let Some(ref vr_sph) = renderer.vr_spheres_renderer {
                                                        vr_sph.render(&mut pass);
                                                    }
                                                    vr.render_controllers(&mut pass, &vr.right_camera_bind_group);
                                                    if menu_vis { vr.render_menu_quad(&mut pass, &vr.right_camera_bind_group); }
                                                }

                                                renderer.queue.submit(std::iter::once(vr_enc.finish()));

                                                // Release swapchain images back to the runtime
                                                if let Some(s) = vr.vr_session.left_swapchain.as_mut() { let _ = s.release_image(); }
                                                if let Some(s) = vr.vr_session.right_swapchain.as_mut() { let _ = s.release_image(); }
                                            }
                                        }
                                        if let Err(e) = vr.end_frame(&frame_state) {
                                            log::warn!("VR end_frame: {}", e);
                                        }
                                    }
                                    Err(e) => log::warn!("VR begin_frame: {}", e),
                                }
                            }
                        }
                    }

                    // Update camera and render 3D scene

                    match renderer.surface.get_current_texture() {
                        Ok(output) => {
                            let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

                            let mut encoder = renderer.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Render Encoder"),
                            });

                            // Render 3D scene - Support multiple models with independent representations
                            // Collect all visible models first
                            let visible_models: Vec<_> = self.model_manager.visible_models().collect();

                            // Render each model in its own pass
                            for (model_idx, model) in visible_models.iter().enumerate() {
                                let protein = &model.trajectory.topology;

                                // Load this model's protein data into renderer
                                // NOTE: Must reload for each model since renderer only holds one model's data at a time
                                renderer.load_protein(protein);
                                renderer.update_visible_instances(protein);

                                // Update atom selection highlighting (only for first model for now)
                                if !self.ui_state.selected_atoms.is_empty() && model_idx == 0 {
                                    renderer.update_selection(protein, &self.ui_state.selected_atoms);
                                }

                                // Set representation from THIS model's setting
                                let model_representation = match model.representation {
                                    RepresentationType::VanDerWaals => mol_render::RepresentationType::VanDerWaals,
                                    RepresentationType::BallAndStick => mol_render::RepresentationType::BallAndStick,
                                    RepresentationType::Ribbon => mol_render::RepresentationType::Ribbon,
                                    RepresentationType::Surface => mol_render::RepresentationType::Surface,
                                    RepresentationType::SurfaceRaymarch => mol_render::RepresentationType::SurfaceRaymarch,
                                    RepresentationType::SphereRaymarch => mol_render::RepresentationType::SphereRaymarch,
                                };

                                renderer.representation = model_representation;

                                // Ensure geometry is loaded for THIS model's representation
                                // This must be done per-model to handle multiple models with same representation
                                match model_representation {
                                    mol_render::RepresentationType::BallAndStick => {
                                        renderer.ensure_ball_stick(protein);
                                    }
                                    mol_render::RepresentationType::Ribbon => {
                                        renderer.ensure_ribbon(protein);
                                    }
                                    mol_render::RepresentationType::Surface => {
                                        renderer.ensure_surface(protein);
                                    }
                                    mol_render::RepresentationType::SurfaceRaymarch => {
                                        renderer.ensure_surface_raymarch(protein);
                                    }
                                    mol_render::RepresentationType::SphereRaymarch => {
                                        renderer.ensure_sphere_raymarch(protein);
                                    }
                                    _ => {}
                                }

                                // Create render pass for this model
                                // First model clears, subsequent models load existing content
                                let load_op = if model_idx == 0 {
                                    wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 0.1,
                                        g: 0.1,
                                        b: 0.1,
                                        a: 1.0,
                                    })
                                } else {
                                    wgpu::LoadOp::Load
                                };

                                let depth_load_op = if model_idx == 0 {
                                    wgpu::LoadOp::Clear(1.0)
                                } else {
                                    wgpu::LoadOp::Load
                                };

                                {
                                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                        label: Some("Model Render Pass"),
                                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                            view: &view,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: load_op,
                                                store: wgpu::StoreOp::Store,
                                            },
                                        })],
                                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                            view: &renderer.depth_view,
                                            depth_ops: Some(wgpu::Operations {
                                                load: depth_load_op,
                                                store: wgpu::StoreOp::Store,
                                            }),
                                            stencil_ops: None,
                                        }),
                                        timestamp_writes: None,
                                        occlusion_query_set: None,
                                    });

                                    render_pass.set_bind_group(0, &renderer.camera_bind_group, &[]);

                                    // Render THIS model with its representation
                                    match model_representation {
                                        mol_render::RepresentationType::VanDerWaals => {
                                            renderer.spheres_renderer.render(&mut render_pass);
                                        }
                                        mol_render::RepresentationType::BallAndStick => {
                                            if let Some(ref ball_stick) = renderer.ball_stick_renderer {
                                                ball_stick.render(&mut render_pass);
                                            }
                                        }
                                        mol_render::RepresentationType::Ribbon => {
                                            if let Some(ref ribbon) = renderer.ribbon_renderer {
                                                ribbon.render(&mut render_pass);
                                            }
                                        }
                                        mol_render::RepresentationType::Surface => {
                                            if let Some(ref surface) = renderer.surface_renderer {
                                                surface.render(&mut render_pass);
                                            }
                                        }
                                        mol_render::RepresentationType::SurfaceRaymarch => {
                                            if let Some(ref surface_rm) = renderer.surface_raymarch_renderer {
                                                surface_rm.render(&mut render_pass);
                                            }
                                        }
                                        mol_render::RepresentationType::SphereRaymarch => {
                                            if let Some(ref sphere_rm) = renderer.sphere_raymarch_renderer {
                                                sphere_rm.render(&mut render_pass);
                                            }
                                        }
                                    }
                                }
                            }

                            // Render coordinate axes in a final pass
                            if self.ui_state.show_axes {
                                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("Axes Render Pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &view,
                                        resolve_target: None,
                                        ops: wgpu::Operations {
                                            load: wgpu::LoadOp::Load,
                                            store: wgpu::StoreOp::Store,
                                        },
                                    })],
                                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                        view: &renderer.depth_view,
                                        depth_ops: Some(wgpu::Operations {
                                            load: wgpu::LoadOp::Load,
                                            store: wgpu::StoreOp::Store,
                                        }),
                                        stencil_ops: None,
                                    }),
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });

                                render_pass.set_bind_group(0, &renderer.camera_bind_group, &[]);
                                renderer.axes_renderer.render(&mut render_pass);
                            }

                            // Render egui
                            let primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
                            let screen_descriptor = egui_wgpu::ScreenDescriptor {
                                size_in_pixels: [renderer.config.width, renderer.config.height],
                                pixels_per_point: window.scale_factor() as f32,
                            };

                            for (id, image_delta) in &full_output.textures_delta.set {
                                egui_renderer.update_texture(&renderer.device, &renderer.queue, *id, image_delta);
                            }

                            egui_renderer.update_buffers(
                                &renderer.device,
                                &renderer.queue,
                                &mut encoder,
                                &primitives,
                                &screen_descriptor,
                            );

                            {
                                let egui_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("Egui Render Pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &view,
                                        resolve_target: None,
                                        ops: wgpu::Operations {
                                            load: wgpu::LoadOp::Load,
                                            store: wgpu::StoreOp::Store,
                                        },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });

                                // egui_wgpu::Renderer::render() requires RenderPass<'static> by design.
                                // Using forget_lifetime() is the documented approach (see egui-wgpu docs).
                                // This is safe because the render pass is properly scoped and the encoder
                                // is not accessed until after the render pass ends.
                                // Ref: https://docs.rs/egui-wgpu/0.29/egui_wgpu/struct.Renderer.html#method.render
                                let mut egui_pass_static = egui_pass.forget_lifetime();
                                egui_renderer.render(&mut egui_pass_static, &primitives, &screen_descriptor);
                            }

                            for id in &full_output.textures_delta.free {
                                egui_renderer.free_texture(id);
                            }

                            renderer.queue.submit(std::iter::once(encoder.finish()));
                            output.present();
                        }
                        Err(wgpu::SurfaceError::Lost) => renderer.resize(renderer.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => log::error!("Render error: {:?}", e),
                    }
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // Only handle mouse input if egui didn't consume it
                if !egui_consumed_event && !self.egui_wants_pointer {
                    match button {
                        MouseButton::Left => {
                            if state == ElementState::Pressed {
                                self.mouse_pressed = true;
                                // Record mouse down position for click detection
                                self.mouse_down_pos = self.last_mouse_pos;
                            } else {
                                self.mouse_pressed = false;

                                // Check if this was a click (mouse up at same position as mouse down)
                                if let (Some(down_pos), Some(up_pos)) = (self.mouse_down_pos, self.last_mouse_pos) {
                                    let dist = ((up_pos.0 - down_pos.0).powi(2) + (up_pos.1 - down_pos.1).powi(2)).sqrt();

                                    // If mouse moved less than 5 pixels, consider it a click
                                    if dist < 5.0 && self.ctrl_pressed {
                                        self.handle_selection_click(up_pos.0 as f32, up_pos.1 as f32);
                                    }
                                }

                                self.mouse_down_pos = None;
                            }
                        }
                        MouseButton::Right => {
                            self.right_mouse_pressed = state == ElementState::Pressed;
                        }
                        _ => {}
                    }
                }
            },

            WindowEvent::CursorMoved { position, .. } => {
                // Only handle cursor movement for camera if egui doesn't want the pointer
                if !self.egui_wants_pointer {
                    if let Some(last_pos) = self.last_mouse_pos {
                        let delta_x = position.x - last_pos.0;
                        let delta_y = position.y - last_pos.1;

                        if let Some(renderer) = &mut self.renderer {
                            if self.mouse_pressed {
                                // Orbit camera
                                let sensitivity = 0.005;
                                renderer
                                    .camera
                                    .orbit(delta_x as f32 * sensitivity, delta_y as f32 * sensitivity);
                            } else if self.right_mouse_pressed {
                                // Pan camera
                                let sensitivity = 0.1;
                                renderer
                                    .camera
                                    .pan(-delta_x as f32 * sensitivity, delta_y as f32 * sensitivity);
                            }
                        }
                    }
                }

                self.last_mouse_pos = Some((position.x, position.y));
            }

            WindowEvent::MouseWheel { delta, .. } => {
                // Only handle mouse wheel if egui didn't consume it
                if !egui_consumed_event && !self.egui_wants_pointer {
                    if let Some(renderer) = &mut self.renderer {
                        let scroll_amount = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y * 5.0,
                            MouseScrollDelta::PixelDelta(pos) => pos.y as f32 * 0.1,
                        };

                        renderer.camera.zoom(scroll_amount);
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// Detect if VR is available on this system by trying to load the OpenXR runtime
fn detect_vr_availability() -> bool {
    // Just check that the OpenXR loader DLL exists and loads — no extensions needed here.
    // The actual session (with khr_vulkan_enable2) is created later in Renderer::new().
    match unsafe { mol_vr::openxr::Entry::load() } {
        Ok(entry) => {
            // Try creating a minimal instance (no graphics extensions) to confirm runtime works
            let no_ext = mol_vr::openxr::ExtensionSet::default();
            match entry.create_instance(
                &mol_vr::openxr::ApplicationInfo {
                    application_name: "PDB Visual VR Detect",
                    application_version: 1,
                    engine_name: "",
                    engine_version: 0,
                    api_version: mol_vr::openxr::Version::new(1, 0, 0),
                },
                &no_ext,
                &[],
            ) {
                Ok(instance) => {
                    match instance.system(mol_vr::openxr::FormFactor::HEAD_MOUNTED_DISPLAY) {
                        Ok(_) => {
                            log::info!("OpenXR runtime detected (Oculus/SteamVR) with HMD");
                            true
                        }
                        Err(e) => {
                            // Runtime is present but no headset active — still try VR,
                            // the session will block until headset is ready
                            log::warn!("OpenXR runtime found but no HMD system yet: {} — attempting anyway", e);
                            true
                        }
                    }
                }
                Err(e) => {
                    log::warn!("OpenXR instance creation failed: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            log::warn!("OpenXR loader not found (openxr_loader.dll missing?): {}", e);
            false
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();

    log::info!("=== PDB Visual - Molecular Viewer ===");
    log::info!("");
    log::info!("Controls:");
    log::info!("  Left mouse drag:  Rotate");
    log::info!("  Right mouse drag: Pan");
    log::info!("  Mouse wheel:      Zoom");
    log::info!("  1: Van der Waals representation");
    log::info!("  2: Ball-and-Stick representation");
    log::info!("  3: Ribbon representation");
    log::info!("  4: Surface representation (marching cubes)");
    log::info!("  5: Surface representation (raymarching - realtime)");
    log::info!("  6: Spheres Realtime (fast raymarched spheres)");
    log::info!("  R: Reset camera");
    log::info!("  U: Toggle UI");
    log::info!("  ESC: Quit");
    log::info!("");

    let args: Vec<String> = env::args().collect();

    // Check for --vr flag
    let vr_flag = args.iter().any(|arg| arg == "--vr");

    // Get protein path (skip --vr flags)
    let protein_path = args.iter()
        .skip(1)
        .find(|arg| !arg.starts_with("--"))
        .cloned();

    if protein_path.is_none() {
        log::warn!("No PDB file provided");
        log::info!("Usage: {} [--vr] <pdb_file>", args[0]);
        log::info!("Example: {} 9PZW.pdb", args[0]);
        log::info!("Example VR: {} --vr 9PZW.pdb", args[0]);
    }

    // Determine VR mode: only check when --vr flag is passed
    let vr_mode = if vr_flag {
        log::info!("VR mode explicitly requested via --vr flag");
        // Check if VR runtime is available
        if detect_vr_availability() {
            log::info!("OpenXR runtime detected, starting in VR mode");
            true
        } else {
            log::error!("VR mode requested but no OpenXR runtime found!");
            log::error!("Please install SteamVR, Oculus Software, or Windows Mixed Reality");
            return Err(anyhow::anyhow!("OpenXR runtime not available"));
        }
    } else {
        // Skip VR auto-detection to avoid loading openxr_loader.dll on PC
        log::debug!("Starting in desktop mode (use --vr flag for VR mode)");
        false
    };

    if vr_mode {
        log::info!("Starting in VR MODE");
        log::info!("VR Controls:");
        log::info!("  Right joystick: Rotate molecule");
        log::info!("  Left joystick:  Move camera");
        log::info!("  Left thumbstick click: Teleport");
        log::info!("  Grip buttons: Select atoms");
        log::info!("");
    } else {
        log::info!("Starting in DESKTOP MODE");
        log::info!("");
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new(protein_path, vr_mode);
    event_loop.run_app(&mut app)?;

    Ok(())
}
