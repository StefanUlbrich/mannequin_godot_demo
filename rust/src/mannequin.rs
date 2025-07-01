use crate::secondary::solve_secondary_goals;
use faer::MatRef;
use godot::prelude::*;
use godot::{
    classes::{
        BoneAttachment3D, ISkeletonModifier3D, RigidBody3D, Skeleton3D, SkeletonModifier3D,
        notify::Node3DNotification,
    },
    global::PropertyHint,
    meta::PropertyInfo,
};
use itertools::Itertools;
use mannequin::{
    Differentiable, DifferentiableModel, NodeLike, arena::iterables::OptimizedDirectionIterable,
    differentiable::Filterable, faer::solve_linear, godot::GodotTree,
};
use std::f32::consts::PI;

#[derive(GodotConvert, Var, Export, Debug)]
#[godot(via = GString)]
pub enum Method {
    Gradient, // first enumerator is default.
    Solve,
    Secondary,
    Orientation,
}

#[allow(dead_code)]
#[derive(GodotClass)]
#[class(tool, base=SkeletonModifier3D)]
struct RsMannequinIK {
    #[export]
    velocity: f32,

    angles: Vec<f32>,

    #[export]
    #[var(get,set = set_main_effector)]
    main_effector: GString,

    #[export]
    #[var(get,set = set_secondary_effector)]
    secondary_effector: GString,

    #[export] // Use when >0.2.4 is released
    main_target: Option<Gd<RigidBody3D>>,

    #[export] // Use when >0.2.4 is released
    default_main_target: Option<Gd<RigidBody3D>>,

    #[export] // Use when >0.2.4 is released
    secondary_target: Option<Gd<RigidBody3D>>,

    #[export] // Use when >0.2.4 is released
    default_secondary_target: Option<Gd<RigidBody3D>>,

    #[export]
    orientation: Option<Gd<BoneAttachment3D>>,

    // #[export]
    // effectors: Array<Gd<Marker3D>>,
    #[export]
    #[var(get,set = set_method)]
    method: Method,

    #[export]
    min_dist: f32,

    base: Base<SkeletonModifier3D>,
    tree: GodotTree,
    differentiable: DifferentiableModel<f32>,
    // indices of active bones
    active_bones: Vec<i32>,
}

impl RsMannequinIK {
    /// Update the mannequin-related structures
    ///
    /// TODO figure out where to call it for hot-reloading
    fn update_mannequin(&mut self) {
        godot_print!(
            "Main: `{}`, secondary: `{}`",
            self.main_effector,
            self.secondary_effector
        );
        if let Some(skeleton) = self.base().get_skeleton() {
            // Use BoneAttachments instead! Select them in the UI

            let main_effector_idx = skeleton.find_bone(&self.main_effector);
            if main_effector_idx == -1 {
                godot_error!("Could not find `{}` in skeleton", self.main_effector);
                return;
            }

            godot_print!("main effector: `{main_effector_idx}`");

            let mut effectors = vec![&main_effector_idx];

            let secondary_effector_idx = skeleton.find_bone(&self.secondary_effector);
            if secondary_effector_idx != -1 {
                godot_print!("secondary effector: `{main_effector_idx}`");
                if matches!(self.method, Method::Secondary) {
                    effectors.push(&secondary_effector_idx);
                }
            } else {
                godot_warn!("Could not find `{}` in skeleton", self.secondary_effector);
            };

            self.active_bones = (0..skeleton.get_bone_count())
                .filter(|idx| skeleton.is_bone_enabled(*idx))
                .collect_vec();

            // Not yet supported

            // Angles must be computed for all joints!
            self.angles = vec![0.0; skeleton.get_bone_count() as usize];

            godot_print!(
                "Active bones (joints): {:?}",
                self.active_bones
                    .iter()
                    .map(|idx| skeleton.get_bone_name(*idx))
                    .collect_vec()
            );

            self.tree = skeleton.into();

            if matches!(self.method, Method::Orientation) {
                self.tree.iter_mut().for_each(|node| {
                    node.get_mut().orientation = true;
                });
            }

            self.tree.iter_mut().for_each(|node| {
                if *node.id() == main_effector_idx
                    || (matches!(self.method, Method::Secondary)
                        && *node.id() == secondary_effector_idx)
                {
                    node.get_mut().effector = true;
                }
            });
            self.tree.iter().for_each(|node| {
                godot_print!(
                    "Created node (depth {}): {:?}. Trafo: {:?}",
                    node.depth(),
                    node.get(),
                    node.id(),
                )
            });
            self.differentiable.setup(
                &self.tree,
                &self.active_bones.iter().collect_vec(),
                &effectors,
            );

            godot_print!("Setup. Jacobian shape: {:?}", self.differentiable.shape());
        } else {
            godot_error!("No skeleton found");
        }
    }
}

#[godot_api]
impl RsMannequinIK {
    /// Changing the end-effectors requires recomputing the tree
    #[func]
    pub fn set_main_effector(&mut self, value: GString) {
        self.main_effector = value;
        self.update_mannequin();
    }

    /// Changing the end-effectors requires recomputing the tree
    #[func]
    pub fn set_secondary_effector(&mut self, value: GString) {
        self.secondary_effector = value;
        self.update_mannequin();
    }

    #[func]
    pub fn set_method(&mut self, value: Method) {
        self.method = value;
        godot_print!("Updated method");
        self.update_mannequin();
    }
}

#[godot_api]
impl ISkeletonModifier3D for RsMannequinIK {
    fn init(base: Base<Self::Base>) -> Self {
        godot_print!("init");

        Self {
            velocity: 0.01,
            angles: vec![],
            main_effector: GString::new(),
            secondary_effector: GString::new(),
            base,
            main_target: None,
            secondary_target: None,
            default_main_target: None,
            default_secondary_target: None,
            tree: GodotTree::new(),
            differentiable: DifferentiableModel::new(),
            method: Method::Gradient,
            active_bones: vec![],
            min_dist: 1.2, // meters
            orientation: None,
        }
    }

    fn on_notification(&mut self, what: Node3DNotification) {
        if what == Node3DNotification::EXTENSION_RELOADED {
            // enabling hot reload
            godot_print!("Extension RsMannekinIK got reloaded");
            self.update_mannequin();
        }
    }

    fn enter_tree(&mut self) {
        self.update_mannequin();
    }

    fn ready(&mut self) {
        self.update_mannequin();
    }

    fn validate_property(&self, property: &mut PropertyInfo) {
        match property.property_name.to_string().as_str() {
            "main_effector" => {
                if let Some(skeleton) = self.base().get_skeleton() {
                    property.hint_info.hint = PropertyHint::ENUM;
                    property.hint_info.hint_string = skeleton.get_concatenated_bone_names().into();
                }
            }
            "secondary_effector" => {
                if let Some(skeleton) = self.base().get_skeleton() {
                    property.hint_info.hint = PropertyHint::ENUM;
                    property.hint_info.hint_string = skeleton.get_concatenated_bone_names().into();
                }
            }
            _ => {}
        }
    }

    fn process_modification(&mut self) {
        let skeleton: Option<Gd<Skeleton3D>> = self.base().get_skeleton();
        if let Some(mut skeleton) = skeleton {
            if let Some(main) = &self.main_target {
                self.differentiable.compute(
                    &self.tree,
                    &self.angles,
                    mannequin::differentiable::ComputeSelection::All,
                );

                // godot_print!("Computed diff {}", diff);

                let jacobian = self.differentiable.jacobian(); // 3x6
                let (rows, cols) = self.differentiable.shape();

                // godot_print!(
                //     "jacobian: ({:?}) {:?}",
                //     self.differentiable.shape(),
                //     jacobian
                // );
                let update = match self.method {
                    Method::Gradient => {
                        let effector: [f32; 3] =
                            self.differentiable.effectors()[0].try_into().unwrap();

                        let mut diff = skeleton.get_global_transform().affine_inverse()
                            * main.get_global_position()
                            - Vector3::from_array(effector);

                        if diff.length() > self.min_dist {
                            if let Some(default_main) = &self.default_main_target {
                                diff = skeleton.get_global_transform().affine_inverse()
                                    * default_main.get_global_position()
                                    - Vector3::from_array(effector)
                            } else {
                                godot_warn!("No default main target")
                            }
                        }

                        let mut update = jacobian
                            .chunks(rows)
                            .map(|col| {
                                diff.dot(Vector3::new(col[0], col[1], col[2])) * self.velocity
                            })
                            .collect_vec();
                        let norm = update.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();

                        let factor = self.velocity.min(norm) / norm;

                        update.iter_mut().map(|x| *x * factor).collect_vec()
                    }

                    Method::Solve => {
                        let mut update = vec![0f32; self.active_bones.len()];
                        let effector: [f32; 3] =
                            self.differentiable.effectors()[0].try_into().unwrap();

                        let mut diff = skeleton.get_global_transform().affine_inverse()
                            * main.get_global_position()
                            - Vector3::from_array(effector);
                        if diff.length() > self.min_dist {
                            if let Some(default_main) = &self.default_main_target {
                                diff = skeleton.get_global_transform().affine_inverse()
                                    * default_main.get_global_position()
                                    - Vector3::from_array(effector)
                            } else {
                                godot_warn!("No default main target")
                            }
                        }

                        solve_linear(
                            jacobian,
                            rows,
                            cols,
                            &diff.to_array(),
                            &mut update,
                            PI / 180.0 * self.velocity,
                        );
                        update
                    }

                    Method::Secondary => {
                        let mut update = vec![0f32; self.active_bones.len()];
                        if let Some(secondary) = &self.secondary_target {
                            let mut effector: [f32; 3] =
                                self.differentiable.effectors()[1].try_into().unwrap();

                            let mut diff_1 = skeleton.get_global_transform().affine_inverse()
                                * main.get_global_position()
                                - Vector3::from_array(effector);

                            if diff_1.length() > self.min_dist {
                                if let Some(default_main) = &self.default_main_target {
                                    diff_1 = skeleton.get_global_transform().affine_inverse()
                                        * default_main.get_global_position()
                                        - Vector3::from_array(effector)
                                } else {
                                    godot_warn!("No default main target")
                                }
                            }

                            effector = self.differentiable.effectors()[0].try_into().unwrap();

                            let mut diff_2 = skeleton.get_global_transform().affine_inverse()
                                * secondary.get_global_position()
                                - Vector3::from_array(effector);

                            if diff_2.length() > self.min_dist {
                                if let Some(default) = &self.default_secondary_target {
                                    diff_2 = skeleton.get_global_transform().affine_inverse()
                                        * default.get_global_position()
                                        - Vector3::from_array(effector)
                                } else {
                                    godot_warn!("No default scnd target")
                                }
                            }

                            solve_secondary_goals(
                                jacobian,
                                rows,
                                cols,
                                &diff_1.to_array(),
                                &diff_2.to_array(),
                                &mut update,
                                PI / 180.0 * self.velocity,
                            );
                        } else {
                            godot_warn!("No secondary target set")
                        }
                        update
                    }
                    Method::Orientation => {
                        let mut update = vec![0f32; self.active_bones.len()];
                        if let Some(orientation) = &self.orientation {
                            // Why are we using a bonemarker? Answer: It is just simpler to use
                            // Godot's forward kinematics for the orientation

                            let effector = skeleton.find_bone(&self.main_effector);

                            // In skeleton coordinates
                            // skeleton.get_bone_global_pose_override(bone_idx)
                            let pose = skeleton.get_bone_global_pose_override(effector);
                            godot_print!("Pose: {:?}", pose);

                            let pose = orientation.get_transform();

                            godot_print!("Orientation object pose: {:?}", pose.origin);

                            // transform target into skeleton coordinates and into bone coordinates
                            let mut relative = pose.affine_inverse()
                                * skeleton.get_global_transform().affine_inverse()
                                * main.get_global_transform();

                            if relative.origin.length() > self.min_dist {
                                if let Some(default_main) = &self.default_main_target {
                                    relative = pose.affine_inverse()
                                        * skeleton.get_global_transform().affine_inverse()
                                        * default_main.get_global_transform();
                                } else {
                                    godot_warn!("No default main target");
                                    unreachable!() //hopefully
                                }
                            }

                            // Convert to quaternions ..
                            let quaternions = relative.basis.get_quaternion();

                            let angle = 2.0 * quaternions.w.acos();

                            godot_print!("Quaternions: {:?}", quaternions);
                            godot_print!("angle: {:?}", angle);

                            let denominator = (1.0 - quaternions.w.powi(2)).sqrt();
                            godot_print!("demnominator {denominator:>}");

                            // This is a scaled axis angle representation favorable for IK
                            let scaled_axis = if denominator > 1e-5 {
                                Vector3::new(quaternions.x, quaternions.y, quaternions.z) * angle
                                    / denominator
                            } else {
                                Vector3::new(0.0, 0.0, 0.0)
                            };

                            godot_print!("Orientation {scaled_axis:?}");

                            // .. get the 6D end effector
                            let effector: [f32; 6] =
                                self.differentiable.effectors()[0].try_into().unwrap();

                            godot_print!("Effector {:?}", &effector[0..3]);

                            // Compute distance and alternatively pick

                            let diff = [
                                relative.origin.x,
                                relative.origin.y,
                                relative.origin.z,
                                scaled_axis.x,
                                scaled_axis.y,
                                scaled_axis.z,
                            ];

                            godot_print!("Diff before solve {diff:?}");

                            let matrix = MatRef::from_column_major_slice(jacobian, rows, cols);
                            godot_print!("Jacobian: {matrix:?}");

                            solve_linear(
                                jacobian,
                                rows,
                                cols,
                                &diff,
                                &mut update,
                                PI / 180.0 * self.velocity,
                            );
                            godot_print!("update: {update:?}");
                        } else {
                            godot_error!(
                                "You need to attach an BoneAttachment3d to the tip first and assign it to the orientation field"
                            );
                        }

                        update
                    }
                };
                // godot_print!("angles: {:?} result: {result:?}", self.angles);

                self.angles
                    .iter_mut()
                    .filter_active(self.differentiable.active())
                    .zip(&update)
                    .for_each(|(angle, update)| {
                        *angle += *update;
                    });

                self.angles.iter().enumerate().for_each(|(idx, angle)| {
                    let mut pose = skeleton.get_bone_pose(idx as i32);
                    pose = pose * Transform3D::IDENTITY.rotated(Vector3::BACK, *angle);
                    skeleton.set_bone_pose(idx as i32, pose);
                });
            }
        }
    }
}
