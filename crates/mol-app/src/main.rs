use anyhow::Result;
use mol_render::{FpsCounter, Renderer, RepresentationType};
use mol_ui::{AtomSelectionInfo, MolecularUI, UIState, RepresentationType as UIRepType};
use mol_vr::openxr;
use pdb_parser::{infer_bonds_optimized, parse_pdb_trajectory, Protein, Trajectory};
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
    trajectory: Option<Trajectory>,
    protein_path: Option<String>,

    // VR mode flag
    vr_mode: bool,
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
            trajectory: None,
            protein_path,
            vr_mode,
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

    fn load_protein_file(&mut self, path: &str) -> Result<()> {
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

        // Update UI state with protein info
        self.ui_state.protein_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
        self.ui_state.atom_count = trajectory.topology.atoms.len();
        self.ui_state.chain_count = trajectory.topology.chains.len();
        self.ui_state.helix_count = trajectory.topology.secondary_structure.helices.len();
        self.ui_state.sheet_count = trajectory.topology.secondary_structure.sheets.len();

        // Update animation state
        self.ui_state.is_animated = trajectory.is_animated();
        self.ui_state.total_frames = trajectory.frame_count();
        self.ui_state.current_frame = 0;
        self.ui_state.playing = false;

        if trajectory.is_animated() {
            log::info!("  Animation detected with {} frames", trajectory.frame_count());
        }

        self.trajectory = Some(trajectory);

        Ok(())
    }

    /// Handle a selection click at the given screen coordinates
    fn handle_selection_click(&mut self, screen_x: f32, screen_y: f32) {
        let (renderer, protein) = match (&self.renderer, &self.trajectory) {
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

            // Update selection based on mode
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
                    Self::select_residue_atoms(protein, atom_idx, &mut self.ui_state.selected_atoms);
                }
                mol_ui::SelectionMode::Chain => {
                    Self::select_chain_atoms(protein, atom_idx, &mut self.ui_state.selected_atoms);
                }
            }

            // Update selection info for UI display
            self.ui_state.selection_info =
                Some(mol_ui::AtomSelectionInfo::from_atom(&protein.atoms[atom_idx], atom_idx));
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
                if let Err(e) = self.load_protein_file(&path) {
                    log::error!("Failed to load PDB file: {}", e);
                }
            } else {
                log::info!("No PDB file provided.");
                log::info!("Usage: pdbvisual <pdb_file>");
            }

            // Initialize renderer
            let mut renderer = pollster::block_on(Renderer::new(window.clone()))
                .expect("Failed to initialize renderer");

            // If trajectory loaded, send it to renderer
            if let Some(ref trajectory) = self.trajectory {
                renderer.load_protein(&trajectory.topology);
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
                        KeyCode::KeyR => {
                            // Reset camera
                            if let Some(ref trajectory) = self.trajectory {
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
                            self.molecular_ui.show_controls = !self.molecular_ui.show_controls;
                            self.molecular_ui.show_info = !self.molecular_ui.show_info;
                            log::info!("UI panels {}", if self.molecular_ui.show_controls { "enabled" } else { "disabled" });
                        }
                        KeyCode::KeyA if self.ctrl_pressed => {
                            // Select all atoms (Ctrl+A)
                            if let Some(ref trajectory) = self.trajectory {
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
                        // Representation changed from UI
                        renderer.representation = match self.ui_state.representation {
                            UIRepType::VanDerWaals => RepresentationType::VanDerWaals,
                            UIRepType::BallStick => RepresentationType::BallAndStick,
                            UIRepType::Ribbon => RepresentationType::Ribbon,
                            UIRepType::Surface => RepresentationType::Surface,
                        };
                        self.ui_state.representation_changed = true;
                        log::info!("Representation changed from UI to {:?}", renderer.representation);
                    }

                    // Handle representation changes - TWO FRAME SYSTEM for loading UI
                    // Frame 1: Detect change, show loading UI
                    // Frame 2: Generate geometry (in next frame so UI renders)

                    if self.ui_state.representation_changed && self.trajectory.is_some() {
                        // First frame: Just mark as pending and show loading UI
                        self.ui_state.pending_representation = Some(self.ui_state.representation);
                        self.ui_state.representation_changed = false;

                        // Set loading message based on representation
                        self.ui_state.loading_message = match self.ui_state.representation {
                            UIRepType::BallStick => "Loading Ball & Stick...".to_string(),
                            UIRepType::Ribbon => "Loading Ribbon...".to_string(),
                            UIRepType::Surface => "Generating Surface (this may take a moment)...".to_string(),
                            UIRepType::VanDerWaals => String::new(),
                        };

                        self.ui_state.loading_representation = match self.ui_state.representation {
                            UIRepType::VanDerWaals => false,
                            _ => true,
                        };
                    }

                    // Second frame: Actually generate the geometry
                    if let Some(pending_rep) = self.ui_state.pending_representation {
                        if let Some(ref trajectory) = self.trajectory {
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
                        if let Some(ref trajectory) = self.trajectory {
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

                    // Update atom selection highlighting
                    if !self.ui_state.selected_atoms.is_empty() {
                        if let Some(ref trajectory) = self.trajectory {
                            let protein = &trajectory.topology;
                            renderer.update_selection(protein, &self.ui_state.selected_atoms);
                        }
                    }

                    // Update visible instances with LOD + frustum culling
                    if let Some(ref trajectory) = self.trajectory {
                        let protein = &trajectory.topology;
                        renderer.update_visible_instances(protein);
                    }

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
                        if let Some(ref trajectory) = self.trajectory {
                            if let Some(frame) = trajectory.frames.get(self.ui_state.current_frame) {
                                renderer.update_atom_positions(&trajectory.topology, &frame.coords);
                            }
                        }
                    }

                    // Update VR controls (joystick, teleport) before camera update
                    let mut vr_grip_selection: Option<mol_vr::Pose> = None;

                    if self.vr_mode {
                        let delta_time = self.fps_counter.frame_time_ms() / 1000.0; // Convert ms to seconds

                        // Get VR renderer if available
                        if let Some(vr_renderer) = &mut renderer.vr_renderer {
                            // Sync input actions
                            if let Ok(()) = vr_renderer.vr_session.input.sync(&vr_renderer.vr_session.session) {
                                // Get controller state
                                if let Ok(controller_state) = vr_renderer.vr_session.input.get_controller_state(
                                    &vr_renderer.vr_session.session,
                                    &vr_renderer.vr_session.stage_space,
                                    openxr::Time::from_nanos(0), // TODO: Use actual predicted display time
                                ) {
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

                                    // Right grip: Select atom (edge trigger)
                                    if controller_state.right_grip_pressed && !self.prev_right_grip {
                                        vr_grip_selection = Some(controller_state.right_pose);
                                    }

                                    // Update grip state for next frame
                                    self.prev_right_grip = controller_state.right_grip_pressed;
                                }
                            }
                        }
                    }

                    // Handle VR grip selection (outside renderer borrow)
                    if let Some(grip_pose) = vr_grip_selection {
                        if let Some(ref trajectory) = self.trajectory {
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

                    // Update camera and render 3D scene
                    renderer.update();

                    match renderer.surface.get_current_texture() {
                        Ok(output) => {
                            let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

                            let mut encoder = renderer.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Render Encoder"),
                            });

                            // Render 3D scene
                            {
                                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("Render Pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &view,
                                        resolve_target: None,
                                        ops: wgpu::Operations {
                                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                                r: 0.1,
                                                g: 0.1,
                                                b: 0.1,
                                                a: 1.0,
                                            }),
                                            store: wgpu::StoreOp::Store,
                                        },
                                    })],
                                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                        view: &renderer.depth_view,
                                        depth_ops: Some(wgpu::Operations {
                                            load: wgpu::LoadOp::Clear(1.0),
                                            store: wgpu::StoreOp::Store,
                                        }),
                                        stencil_ops: None,
                                    }),
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });

                                render_pass.set_bind_group(0, &renderer.camera_bind_group, &[]);

                                match renderer.representation {
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
                                }
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

/// Detect if VR is available on this system
fn detect_vr_availability() -> bool {
    // Try to create OpenXR instance
    match mol_vr::VrSession::new() {
        Ok(_) => {
            log::info!("OpenXR VR runtime detected");
            true
        }
        Err(e) => {
            log::debug!("OpenXR not available: {}", e);
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
    log::info!("  4: Surface representation");
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

    // Determine VR mode: explicit flag or auto-detect
    let vr_mode = if vr_flag {
        log::info!("VR mode explicitly requested via --vr flag");
        true
    } else if detect_vr_availability() {
        log::info!("VR runtime detected, but starting in desktop mode");
        log::info!("Use --vr flag to start in VR mode");
        false
    } else {
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
