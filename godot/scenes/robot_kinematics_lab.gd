extends Node3D

var xr_interface: XRInterface


func _ready() -> void:
	xr_interface = XRServer.find_interface("OpenXR")
	if xr_interface and xr_interface.is_initialized():
		DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
		get_viewport().use_xr = true
	else:
		print("OpenXR not initialized, please check if your headset is connected")		

func _on_controller_left_button_pressed(name: String) -> void:
	pass


func _on_controller_left_button_released(name: String) -> void:
	print("Left button released")
	if name == "by_button":
		var object = find_child("Sword")
		object.reset = true
		
		object = find_child("Green")
		object.reset = true
		
		object = find_child("Blue2")
		object.reset = true
		
		
