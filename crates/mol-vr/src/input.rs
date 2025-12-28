//! VR Input System with OpenXR Actions
//!
//! This module handles VR controller input using the OpenXR action system,
//! including pose tracking, button presses, and joystick input.

use anyhow::Result;
use glam::{Quat, Vec2, Vec3};
use log::info;
use openxr as xr;

/// Controller pose (position and orientation)
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    pub position: Vec3,
    pub orientation: Quat,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        }
    }
}

/// Complete state of VR controllers
#[derive(Debug, Clone)]
pub struct ControllerState {
    pub left_pose: Pose,
    pub right_pose: Pose,
    pub left_joystick: Vec2,
    pub right_joystick: Vec2,
    pub left_grip_pressed: bool,
    pub right_grip_pressed: bool,
    pub left_trigger_value: f32,
    pub right_trigger_value: f32,
    pub teleport_pressed: bool,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            left_pose: Pose::default(),
            right_pose: Pose::default(),
            left_joystick: Vec2::ZERO,
            right_joystick: Vec2::ZERO,
            left_grip_pressed: false,
            right_grip_pressed: false,
            left_trigger_value: 0.0,
            right_trigger_value: 0.0,
            teleport_pressed: false,
        }
    }
}

/// VR input manager with OpenXR actions
pub struct VrInput {
    action_set: xr::ActionSet,

    // Pose actions (stored for OpenXR lifecycle but accessed via spaces)
    _left_hand_pose_action: xr::Action<xr::Posef>,
    _right_hand_pose_action: xr::Action<xr::Posef>,

    // Button actions (boolean)
    left_grip_action: xr::Action<bool>,
    right_grip_action: xr::Action<bool>,
    teleport_action: xr::Action<bool>,

    // Analog actions (float)
    left_trigger_action: xr::Action<f32>,
    right_trigger_action: xr::Action<f32>,

    // Joystick actions (Vec2f)
    left_joystick_action: xr::Action<xr::Vector2f>,
    right_joystick_action: xr::Action<xr::Vector2f>,

    // Spaces for pose tracking
    left_hand_space: xr::Space,
    right_hand_space: xr::Space,
}

impl VrInput {
    /// Create a new VR input system
    pub fn new(instance: &xr::Instance, session: &xr::Session<xr::Vulkan>) -> Result<Self> {
        info!("Initializing VR input system...");

        // Create action set
        let action_set = instance.create_action_set("main_actions", "Main Actions", 0)?;

        // Create pose actions
        let left_hand_pose_action = action_set.create_action::<xr::Posef>(
            "left_hand_pose",
            "Left Hand Pose",
            &[],
        )?;

        let right_hand_pose_action = action_set.create_action::<xr::Posef>(
            "right_hand_pose",
            "Right Hand Pose",
            &[],
        )?;

        // Create button actions
        let left_grip_action = action_set.create_action::<bool>(
            "left_grip",
            "Left Grip Button",
            &[],
        )?;

        let right_grip_action = action_set.create_action::<bool>(
            "right_grip",
            "Right Grip Button",
            &[],
        )?;

        let teleport_action = action_set.create_action::<bool>(
            "teleport",
            "Teleport",
            &[],
        )?;

        // Create analog actions
        let left_trigger_action = action_set.create_action::<f32>(
            "left_trigger",
            "Left Trigger",
            &[],
        )?;

        let right_trigger_action = action_set.create_action::<f32>(
            "right_trigger",
            "Right Trigger",
            &[],
        )?;

        // Create joystick actions
        let left_joystick_action = action_set.create_action::<xr::Vector2f>(
            "left_joystick",
            "Left Joystick",
            &[],
        )?;

        let right_joystick_action = action_set.create_action::<xr::Vector2f>(
            "right_joystick",
            "Right Joystick",
            &[],
        )?;

        // Suggest bindings for Oculus Touch controllers
        let oculus_profile = instance.string_to_path("/interaction_profiles/oculus/touch_controller")?;

        instance.suggest_interaction_profile_bindings(
            oculus_profile,
            &[
                // Left hand pose
                xr::Binding::new(
                    &left_hand_pose_action,
                    instance.string_to_path("/user/hand/left/input/grip/pose")?,
                ),
                // Right hand pose
                xr::Binding::new(
                    &right_hand_pose_action,
                    instance.string_to_path("/user/hand/right/input/grip/pose")?,
                ),
                // Left grip
                xr::Binding::new(
                    &left_grip_action,
                    instance.string_to_path("/user/hand/left/input/squeeze/click")?,
                ),
                // Right grip
                xr::Binding::new(
                    &right_grip_action,
                    instance.string_to_path("/user/hand/right/input/squeeze/click")?,
                ),
                // Left trigger
                xr::Binding::new(
                    &left_trigger_action,
                    instance.string_to_path("/user/hand/left/input/trigger/value")?,
                ),
                // Right trigger
                xr::Binding::new(
                    &right_trigger_action,
                    instance.string_to_path("/user/hand/right/input/trigger/value")?,
                ),
                // Left joystick
                xr::Binding::new(
                    &left_joystick_action,
                    instance.string_to_path("/user/hand/left/input/thumbstick")?,
                ),
                // Right joystick
                xr::Binding::new(
                    &right_joystick_action,
                    instance.string_to_path("/user/hand/right/input/thumbstick")?,
                ),
                // Teleport (left thumbstick click)
                xr::Binding::new(
                    &teleport_action,
                    instance.string_to_path("/user/hand/left/input/thumbstick/click")?,
                ),
            ],
        )?;

        // Attach action set to session
        session.attach_action_sets(&[&action_set])?;

        // Create action spaces for pose tracking
        let left_hand_space = left_hand_pose_action.create_space(
            session.clone(),
            xr::Path::NULL,
            xr::Posef::IDENTITY,
        )?;

        let right_hand_space = right_hand_pose_action.create_space(
            session.clone(),
            xr::Path::NULL,
            xr::Posef::IDENTITY,
        )?;

        info!("VR input system initialized successfully");
        info!("  - Configured for Oculus Touch controllers");
        info!("  - Actions: poses, grips, triggers, joysticks, teleport");

        Ok(Self {
            action_set,
            _left_hand_pose_action: left_hand_pose_action,
            _right_hand_pose_action: right_hand_pose_action,
            left_grip_action,
            right_grip_action,
            teleport_action,
            left_trigger_action,
            right_trigger_action,
            left_joystick_action,
            right_joystick_action,
            left_hand_space,
            right_hand_space,
        })
    }

    /// Synchronize action states (call once per frame)
    pub fn sync(&self, session: &xr::Session<xr::Vulkan>) -> Result<()> {
        session.sync_actions(&[xr::ActiveActionSet::new(&self.action_set)])?;
        Ok(())
    }

    /// Get current controller state
    pub fn get_controller_state(
        &self,
        session: &xr::Session<xr::Vulkan>,
        reference_space: &xr::Space,
        predicted_display_time: xr::Time,
    ) -> Result<ControllerState> {
        let mut state = ControllerState::default();

        // Get left hand pose
        if let Ok(location) = self.left_hand_space.locate(reference_space, predicted_display_time) {
            if location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID)
                && location.location_flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
            {
                state.left_pose = Pose {
                    position: Vec3::new(
                        location.pose.position.x,
                        location.pose.position.y,
                        location.pose.position.z,
                    ),
                    orientation: Quat::from_xyzw(
                        location.pose.orientation.x,
                        location.pose.orientation.y,
                        location.pose.orientation.z,
                        location.pose.orientation.w,
                    ),
                };
            }
        }

        // Get right hand pose
        if let Ok(location) = self.right_hand_space.locate(reference_space, predicted_display_time) {
            if location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID)
                && location.location_flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
            {
                state.right_pose = Pose {
                    position: Vec3::new(
                        location.pose.position.x,
                        location.pose.position.y,
                        location.pose.position.z,
                    ),
                    orientation: Quat::from_xyzw(
                        location.pose.orientation.x,
                        location.pose.orientation.y,
                        location.pose.orientation.z,
                        location.pose.orientation.w,
                    ),
                };
            }
        }

        // Get button states
        state.left_grip_pressed = self.left_grip_action
            .state(session, xr::Path::NULL)
            .map(|s| s.current_state)
            .unwrap_or(false);

        state.right_grip_pressed = self.right_grip_action
            .state(session, xr::Path::NULL)
            .map(|s| s.current_state)
            .unwrap_or(false);

        state.teleport_pressed = self.teleport_action
            .state(session, xr::Path::NULL)
            .map(|s| s.current_state)
            .unwrap_or(false);

        // Get trigger values
        state.left_trigger_value = self.left_trigger_action
            .state(session, xr::Path::NULL)
            .map(|s| s.current_state)
            .unwrap_or(0.0);

        state.right_trigger_value = self.right_trigger_action
            .state(session, xr::Path::NULL)
            .map(|s| s.current_state)
            .unwrap_or(0.0);

        // Get joystick values
        if let Ok(joystick_state) = self.left_joystick_action.state(session, xr::Path::NULL) {
            state.left_joystick = Vec2::new(
                joystick_state.current_state.x,
                joystick_state.current_state.y,
            );
        }

        if let Ok(joystick_state) = self.right_joystick_action.state(session, xr::Path::NULL) {
            state.right_joystick = Vec2::new(
                joystick_state.current_state.x,
                joystick_state.current_state.y,
            );
        }

        Ok(state)
    }

    /// Get forward direction vector from controller pose
    pub fn get_controller_forward(pose: &Pose) -> Vec3 {
        // In OpenXR, forward is -Z axis after rotation
        pose.orientation * Vec3::new(0.0, 0.0, -1.0)
    }

    /// Get right direction vector from controller pose
    pub fn get_controller_right(pose: &Pose) -> Vec3 {
        // In OpenXR, right is +X axis after rotation
        pose.orientation * Vec3::new(1.0, 0.0, 0.0)
    }

    /// Get up direction vector from controller pose
    pub fn get_controller_up(pose: &Pose) -> Vec3 {
        // In OpenXR, up is +Y axis after rotation
        pose.orientation * Vec3::new(0.0, 1.0, 0.0)
    }
}
