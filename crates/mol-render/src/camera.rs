use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3, Vec4, Vec4Swizzles};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub view_pos: [f32; 3],
    pub _padding: f32,
}

/// Field-of-view configuration for asymmetric frustum (VR)
#[derive(Debug, Clone, Copy)]
pub struct FovConfig {
    pub angle_left: f32,
    pub angle_right: f32,
    pub angle_up: f32,
    pub angle_down: f32,
}

/// Stereo camera configuration for VR rendering
#[derive(Debug, Clone, Copy)]
pub struct StereoConfig {
    pub ipd: f32,  // Inter-pupillary distance in meters (typical: 0.063m)
    pub left_fov: FovConfig,
    pub right_fov: FovConfig,
}

/// Eye selector for stereo rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eye {
    Left,
    Right,
}

pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,

    // VR stereo support
    pub stereo_config: Option<StereoConfig>,
}

impl Camera {
    pub fn new(aspect: f32) -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 100.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov: 45.0_f32.to_radians(),
            aspect,
            near: 0.1,
            far: 1000.0,
            stereo_config: None,
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.position, self.target, self.up)
    }

    pub fn projection_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov, self.aspect, self.near, self.far)
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection_matrix() * self.view_matrix()
    }

    pub fn uniform(&self) -> CameraUniform {
        CameraUniform {
            view_proj: self.view_projection().to_cols_array_2d(),
            view_pos: self.position.into(),
            _padding: 0.0,
        }
    }

    pub fn orbit(&mut self, delta_yaw: f32, delta_pitch: f32) {
        let radius = self.position.distance(self.target);
        let offset = self.position - self.target;

        // Convert to spherical coordinates
        let mut phi = offset.z.atan2(offset.x);
        let mut theta = (offset.y / radius).acos();

        // Apply rotation
        phi += delta_yaw;
        theta = (theta + delta_pitch).clamp(0.01, std::f32::consts::PI - 0.01);

        // Convert back to Cartesian
        let x = radius * theta.sin() * phi.cos();
        let y = radius * theta.cos();
        let z = radius * theta.sin() * phi.sin();

        self.position = self.target + Vec3::new(x, y, z);
    }

    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let forward = (self.target - self.position).normalize();
        let right = forward.cross(self.up).normalize();
        let up = right.cross(forward);

        let offset = right * delta_x + up * delta_y;
        self.position += offset;
        self.target += offset;
    }

    pub fn zoom(&mut self, delta: f32) {
        let direction = (self.target - self.position).normalize();
        self.position += direction * delta;
    }

    pub fn look_at(&mut self, target: Vec3) {
        let distance = self.position.distance(self.target);
        self.target = target;

        let direction = (self.target - self.position).normalize();
        self.position = self.target - direction * distance;
    }

    /// Convert screen coordinates to a ray in world space
    ///
    /// # Arguments
    /// * `screen_x` - X coordinate in screen space (0 = left edge)
    /// * `screen_y` - Y coordinate in screen space (0 = top edge)
    /// * `screen_width` - Width of the screen in pixels
    /// * `screen_height` - Height of the screen in pixels
    ///
    /// # Returns
    /// A `Ray` starting from the camera position and pointing through the screen coordinates
    ///
    /// # Algorithm
    /// 1. Convert screen coordinates to Normalized Device Coordinates (NDC) [-1, 1]
    /// 2. Create clip space points at near and far planes
    /// 3. Transform to world space using inverse view-projection matrix
    /// 4. Perform perspective divide (w-division)
    /// 5. Construct ray from camera through the unprojected point
    pub fn screen_to_ray(
        &self,
        screen_x: f32,
        screen_y: f32,
        screen_width: f32,
        screen_height: f32,
    ) -> mol_core::Ray {
        // Convert screen coordinates to NDC [-1, 1]
        // X: left edge (0) -> -1, right edge (width) -> 1
        // Y: top edge (0) -> 1, bottom edge (height) -> -1 (flip Y for screen coords)
        let ndc_x = (screen_x / screen_width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_y / screen_height) * 2.0;

        // Get inverse view-projection matrix
        let inv_view_proj = self.view_projection().inverse();

        // Create clip space points (NDC with depth)
        // Near plane: z = -1 in NDC (after perspective divide)
        // Far plane: z = 1 in NDC
        let near_point_clip = Vec4::new(ndc_x, ndc_y, -1.0, 1.0);
        let far_point_clip = Vec4::new(ndc_x, ndc_y, 1.0, 1.0);

        // Transform to world space
        let near_point_world = inv_view_proj * near_point_clip;
        let far_point_world = inv_view_proj * far_point_clip;

        // Perspective divide to get actual 3D coordinates
        let near_point = near_point_world.xyz() / near_point_world.w;
        let far_point = far_point_world.xyz() / far_point_world.w;

        // Create ray from camera position through the near point
        // Direction is from near point to far point
        let direction = (far_point - near_point).normalize();

        mol_core::Ray {
            origin: self.position,
            direction,
        }
    }

    // ========== VR Stereo Rendering Methods ==========

    /// Get the right vector of the camera
    pub fn right_vector(&self) -> Vec3 {
        let forward = (self.target - self.position).normalize();
        forward.cross(self.up).normalize()
    }

    /// Get stereo eye position with IPD offset
    pub fn eye_position(&self, eye: Eye) -> Vec3 {
        if let Some(stereo) = &self.stereo_config {
            let right = self.right_vector();
            let offset = match eye {
                Eye::Left => right * (-stereo.ipd / 2.0),
                Eye::Right => right * (stereo.ipd / 2.0),
            };
            self.position + offset
        } else {
            // No stereo config, return center position
            self.position
        }
    }

    /// Get view matrix for stereo rendering (left or right eye)
    pub fn view_matrix_stereo(&self, eye: Eye) -> Mat4 {
        let eye_position = self.eye_position(eye);
        Mat4::look_at_rh(eye_position, self.target, self.up)
    }

    /// Create asymmetric projection matrix from FOV configuration
    fn projection_matrix_asymmetric(fov: &FovConfig, near: f32, far: f32) -> Mat4 {
        // Convert angles to tangents for frustum calculation
        let left = near * fov.angle_left.tan();
        let right = near * fov.angle_right.tan();
        let bottom = near * fov.angle_down.tan();
        let top = near * fov.angle_up.tan();

        // Create asymmetric frustum projection matrix
        // This is the standard OpenGL perspective matrix formula
        let width = right - left;
        let height = top - bottom;
        let depth = far - near;

        Mat4::from_cols(
            Vec4::new(2.0 * near / width, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0 * near / height, 0.0, 0.0),
            Vec4::new(
                (right + left) / width,
                (top + bottom) / height,
                -(far + near) / depth,
                -1.0,
            ),
            Vec4::new(0.0, 0.0, -2.0 * far * near / depth, 0.0),
        )
    }

    /// Get projection matrix for stereo rendering (left or right eye)
    pub fn projection_matrix_stereo(&self, eye: Eye) -> Mat4 {
        if let Some(stereo) = &self.stereo_config {
            let fov = match eye {
                Eye::Left => &stereo.left_fov,
                Eye::Right => &stereo.right_fov,
            };
            Self::projection_matrix_asymmetric(fov, self.near, self.far)
        } else {
            // Fallback to standard projection
            self.projection_matrix()
        }
    }

    /// Get combined view-projection matrix for stereo rendering
    pub fn view_projection_stereo(&self, eye: Eye) -> Mat4 {
        self.projection_matrix_stereo(eye) * self.view_matrix_stereo(eye)
    }

    /// Get camera uniform for stereo rendering
    pub fn uniform_stereo(&self, eye: Eye) -> CameraUniform {
        CameraUniform {
            view_proj: self.view_projection_stereo(eye).to_cols_array_2d(),
            view_pos: self.eye_position(eye).into(),
            _padding: 0.0,
        }
    }

    // ========== VR Input Control Methods ==========

    /// Apply VR joystick rotation (orbit around target)
    pub fn apply_vr_rotation(&mut self, joystick: Vec2, delta_time: f32) {
        let sensitivity = 2.0; // Rotation speed
        let yaw = joystick.x * sensitivity * delta_time;
        let pitch = joystick.y * sensitivity * delta_time;
        self.orbit(yaw, pitch);
    }

    /// Apply VR joystick movement (translate camera and target)
    pub fn apply_vr_movement(&mut self, joystick: Vec2, delta_time: f32) {
        let speed = 10.0; // Angstroms per second
        let right = self.right_vector();
        let forward = (self.target - self.position).normalize();

        let offset = (right * joystick.x + forward * joystick.y) * speed * delta_time;
        self.position += offset;
        self.target += offset;
    }

    /// Apply VR teleport (jump to new position based on controller direction)
    pub fn apply_vr_teleport(&mut self, controller_forward: Vec3, distance: f32) {
        let new_target = self.target + controller_forward * distance;
        self.look_at(new_target);
    }
}
