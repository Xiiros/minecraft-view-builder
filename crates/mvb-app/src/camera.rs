//! Orbit camera of the 3D view: always looks at a target point, from a direction (yaw, pitch)
//! and a distance.

use glam::camera::rh::proj::directx;
use glam::camera::rh::view::look_at_mat4;
use glam::{Mat4, Vec2, Vec3};

/// Vertical field of view.
const FOV_Y: f32 = std::f32::consts::FRAC_PI_4;
/// Pitch stays just short of straight up or down, where the view direction would flip.
const MAX_PITCH: f32 = 1.55;
/// Radians per point of mouse movement.
const ORBIT_SPEED: f32 = 0.008;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Angle around the vertical axis; 0 looks from the south (+z) towards the north.
    pub yaw: f32,
    /// Angle above the horizon.
    pub pitch: f32,
    pub distance: f32,
    /// Radius of the scene, used to bound zoom and the depth range.
    radius: f32,
}

impl OrbitCamera {
    /// Looks at the centre of a box of `size` blocks from the south-east, slightly above, with
    /// the whole box in view.
    pub fn fit(size: [u32; 3]) -> Self {
        let size = Vec3::new(size[0] as f32, size[1] as f32, size[2] as f32);
        let radius = (size.length() / 2.0).max(1.0);
        Self {
            target: size / 2.0,
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: 0.6,
            distance: radius / (FOV_Y / 2.0).sin() * 1.1,
            radius,
        }
    }

    /// Unit vector from the target towards the eye.
    fn backward(&self) -> Vec3 {
        Vec3::new(
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.cos(),
        )
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.backward() * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let near = (self.distance * 0.01).max(0.05);
        let far = self.distance + self.radius * 3.0 + 10.0;
        directx::perspective(FOV_Y, aspect.max(1e-3), near, far)
            * look_at_mat4(self.eye(), self.target, Vec3::Y)
    }

    /// Turns around the target by a mouse movement in points.
    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ORBIT_SPEED;
        self.pitch = (self.pitch + delta.y * ORBIT_SPEED).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Moves the target so that the scene follows the mouse, for a view `height` points tall.
    pub fn pan(&mut self, delta: Vec2, height: f32) {
        let units_per_point = 2.0 * self.distance * (FOV_Y / 2.0).tan() / height.max(1.0);
        let forward = -self.backward();
        let right = forward.cross(Vec3::Y).normalize();
        let up = right.cross(forward);
        self.target += (-right * delta.x + up * delta.y) * units_per_point;
    }

    /// Multiplies the distance to the target (below 1 moves closer).
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(0.5, self.radius * 20.0 + 10.0);
    }

    /// Ray through a point of the view, in normalised device coordinates (-1..1, y up).
    pub fn ray(&self, ndc: Vec2, aspect: f32) -> (Vec3, Vec3) {
        let inverse = self.view_proj(aspect).inverse();
        let near = inverse.project_point3(ndc.extend(0.0));
        let far = inverse.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn fitted_camera_looks_at_the_centre() {
        let camera = OrbitCamera::fit([10, 4, 6]);
        assert_eq!(camera.target, Vec3::new(5.0, 2.0, 3.0));
        assert!(((camera.eye() - camera.target).length() - camera.distance).abs() < 1e-3);
        let centre = camera.view_proj(1.5).project_point3(camera.target);
        assert!(centre.x.abs() < 1e-4 && centre.y.abs() < 1e-4);
        // Viewed from the south-east and from above.
        let eye = camera.eye();
        assert!(eye.x > camera.target.x && eye.z > camera.target.z && eye.y > camera.target.y);
    }

    #[test]
    fn ray_through_the_centre_hits_the_target() {
        let camera = OrbitCamera::fit([8, 8, 8]);
        let (origin, direction) = camera.ray(Vec2::ZERO, 1.0);
        let along = (camera.target - origin).dot(direction);
        assert!(close(origin + direction * along, camera.target));
    }

    #[test]
    fn pitch_is_bounded() {
        let mut camera = OrbitCamera::fit([4, 4, 4]);
        camera.orbit(Vec2::new(0.0, 10_000.0));
        assert_eq!(camera.pitch, MAX_PITCH);
        camera.orbit(Vec2::new(0.0, -20_000.0));
        assert_eq!(camera.pitch, -MAX_PITCH);
    }

    #[test]
    fn pan_moves_the_target_sideways_only() {
        let mut camera = OrbitCamera::fit([4, 4, 4]);
        let before = camera;
        camera.pan(Vec2::new(30.0, 0.0), 600.0);
        let moved = camera.target - before.target;
        assert!(moved.length() > 0.0);
        // Perpendicular to the viewing direction, distance unchanged.
        assert!(moved.dot(before.backward()).abs() < 1e-4);
        assert_eq!(camera.distance, before.distance);
    }

    #[test]
    fn zoom_is_bounded() {
        let mut camera = OrbitCamera::fit([4, 4, 4]);
        camera.zoom(1e-6);
        assert_eq!(camera.distance, 0.5);
        camera.zoom(1e9);
        assert!(camera.distance <= camera.radius * 20.0 + 10.0);
    }
}
