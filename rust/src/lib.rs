use godot::prelude::*;

pub mod mannequin;
pub mod secondary;

struct MyExtension;

#[gdextension]
unsafe impl ExtensionLibrary for MyExtension {}
