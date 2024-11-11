use super::*;

use std::{collections::HashSet, error, ffi::CStr, fmt::Display, os::raw::c_void};

use ash::*;
use log::*;

use raw_window_handle::RawDisplayHandle;

const PORTABILITY_MACOS_VERSION: u32 = vk::make_api_version(0, 1, 3, 216);

const VALIDATION_ENABLED: bool = cfg!(debug_assertions);
const VALIDATION_LAYER_NAME: &CStr = c"VK_LAYER_KHRONOS_validation";

impl Error {
    fn new(msg: String) -> Self {
        Self {
            backend: "Vulkan",
            msg,
        }
    }
}
impl From<vk::Result> for Error {
    fn from(value: vk::Result) -> Self {
        Self::new(value.to_string())
    }
}

pub struct Vulkan {
    entry: Entry,
    instance: Instance,
    debug_utils_messenger: Option<vk::DebugUtilsMessengerEXT>,
    surface: Option<vk::SurfaceKHR>,
    device: Option<Device>,
    physical_device: Option<vk::PhysicalDevice>,
    graphics_queue: Option<vk::Queue>,
    present_queue: Option<vk::Queue>,
}

impl Backend for Vulkan {
    fn new(display_handle: DisplayHandle) -> Result<Self, super::Error> {
        let entry = Entry::linked();
        let (instance, debug_utils_messenger) = create_instance(display_handle.as_raw(), &entry)?;

        Ok(Self {
            entry,
            instance,
            debug_utils_messenger,
            surface: None,
            device: None,
            physical_device: None,
            graphics_queue: None,
            present_queue: None,
        })
    }

    fn destroy(&mut self) {
        if let Some(debug_utils_messenger) = self.debug_utils_messenger {
            let debug_utils_loader = ext::debug_utils::Instance::new(&self.entry, &self.instance);
            unsafe {
                debug_utils_loader.destroy_debug_utils_messenger(debug_utils_messenger, None);
            }
        }

        if let Some(device) = &self.device {
            unsafe {
                device.destroy_device(None);
            }
        }

        if let Some(surface) = self.surface {
            unsafe {
                khr::surface::Instance::new(&self.entry, &self.instance)
                    .destroy_surface(surface, None);
            }
        }

        unsafe {
            self.instance.destroy_instance(None);
        }
    }
}

impl Vulkan {
    pub fn create_surface(
        &mut self,
        display_handle: DisplayHandle,
        window_handle: WindowHandle,
    ) -> Result<(), Error> {
        self.surface = Some(unsafe {
            ash_window::create_surface(
                &self.entry,
                &self.instance,
                display_handle.as_raw(),
                window_handle.as_raw(),
                None,
            )?
        });

        Ok(())
    }

    pub fn create_device(&mut self) -> Result<(), Error> {
        let surface = self.surface.ok_or(Error::new(
            "Can't create device without a surface".to_string(),
        ))?;
        let instance_version =
            unsafe { self.entry.try_enumerate_instance_version()? }.unwrap_or(vk::API_VERSION_1_0);

        let physical_device = pick_physical_device(&self.entry, &self.instance, surface)?;

        let indices =
            QueueFamilyIndices::get(&self.entry, &self.instance, surface, physical_device).unwrap();

        let mut unique_indices = HashSet::new();
        unique_indices.insert(indices.graphics);
        unique_indices.insert(indices.present);

        let queue_priorities = &[1.0];
        let queue_infos = unique_indices
            .iter()
            .map(|i| vk::DeviceQueueCreateInfo {
                queue_family_index: *i,
                queue_count: queue_priorities.len() as u32,
                p_queue_priorities: queue_priorities.as_ptr(),
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let mut extensions = vec![];

        // required for mac since 1.3.216
        if cfg!(target_os = "macos") && instance_version >= PORTABILITY_MACOS_VERSION {
            extensions.push(vk::KHR_PORTABILITY_SUBSET_NAME.as_ptr());
        }

        let features = vk::PhysicalDeviceFeatures::default();

        let create_info = vk::DeviceCreateInfo {
            queue_create_info_count: queue_infos.len() as u32,
            p_queue_create_infos: queue_infos.as_ptr(),
            enabled_extension_count: extensions.len() as u32,
            pp_enabled_extension_names: extensions.as_ptr(),
            p_enabled_features: &features,
            ..Default::default()
        };

        let device = unsafe {
            self.instance
                .create_device(physical_device, &create_info, None)?
        };

        unsafe {
            self.graphics_queue = Some(device.get_device_queue(indices.graphics, 0));
            self.present_queue = Some(device.get_device_queue(indices.present, 0));
        }

        self.device = Some(device);
        self.physical_device = Some(physical_device);

        Ok(())
    }
}

pub fn create_instance(
    rdh: RawDisplayHandle,
    entry: &Entry,
) -> Result<(Instance, Option<vk::DebugUtilsMessengerEXT>), super::Error> {
    let instance_version = unsafe {
        entry
            .try_enumerate_instance_version()?
            .unwrap_or(vk::API_VERSION_1_0)
    };

    let app_info = vk::ApplicationInfo {
        p_application_name: b"Vulkan App\0".as_ptr() as *const i8,
        application_version: vk::make_api_version(0, 1, 0, 0),
        p_engine_name: "w_\0".as_ptr() as *const i8,
        engine_version: vk::make_api_version(0, 1, 0, 0),
        api_version: vk::make_api_version(0, 1, 0, 0),
        ..Default::default()
    };

    let mut extensions = ash_window::enumerate_required_extensions(rdh)?.to_owned();

    if VALIDATION_ENABLED {
        extensions.push(vk::EXT_DEBUG_UTILS_NAME.as_ptr());
    }

    let flags = if cfg!(target_os = "macos") && instance_version >= PORTABILITY_MACOS_VERSION {
        info!("Enabling extensions for macOS portability");

        extensions.push(vk::KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_NAME.as_ptr());
        extensions.push(vk::KHR_PORTABILITY_ENUMERATION_NAME.as_ptr());

        vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR
    } else {
        vk::InstanceCreateFlags::empty()
    };

    let instance_layer_properties = unsafe { entry.enumerate_instance_layer_properties()? };
    let available_layers = instance_layer_properties
        .iter()
        .map(|l| l.layer_name_as_c_str().expect("Invalid vulkan layer name."))
        .collect::<HashSet<_>>();

    if VALIDATION_ENABLED && !available_layers.contains(VALIDATION_LAYER_NAME) {
        return Err(Error::new(
            "Validation layers requested but not supported.".to_string(),
        ));
    }

    let layers = if VALIDATION_ENABLED {
        info!("Enabling validation layers");
        vec![VALIDATION_LAYER_NAME.as_ptr()]
    } else {
        Vec::new()
    };

    let mut instance_info = vk::InstanceCreateInfo {
        p_application_info: &app_info,
        enabled_extension_count: extensions.len() as u32,
        pp_enabled_extension_names: extensions.as_ptr(),
        flags,
        enabled_layer_count: layers.len() as u32,
        pp_enabled_layer_names: layers.as_ptr(),
        ..Default::default()
    };

    let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT {
        message_severity: vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
            | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
            | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
            | vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE,
        message_type: vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        pfn_user_callback: Some(debug_callback),
        ..Default::default()
    };

    if VALIDATION_ENABLED {
        instance_info = instance_info.push_next(&mut debug_info);
    }

    let instance = unsafe { entry.create_instance(&instance_info, None)? };
    let debug_utils_messenger = if VALIDATION_ENABLED {
        let debug_utils_loader = ext::debug_utils::Instance::new(&entry, &instance);
        unsafe { Some(debug_utils_loader.create_debug_utils_messenger(&debug_info, None)?) }
    } else {
        None
    };

    Ok((instance, debug_utils_messenger))
}

#[derive(Copy, Clone, Debug)]
struct QueueFamilyIndices {
    graphics: u32,
    present: u32,
}

impl QueueFamilyIndices {
    fn get(
        entry: &Entry,
        instance: &Instance,
        surface: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
    ) -> Result<Self, SuitabilityError> {
        let properties =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

        let graphics = properties
            .iter()
            .position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|i| i as u32);

        let mut present = None;
        for (index, _properties) in properties.iter().enumerate() {
            if unsafe {
                khr::surface::Instance::new(entry, instance)
                    .get_physical_device_surface_support(physical_device, index as u32, surface)
                    .unwrap_or(false)
            } {
                present = Some(index as u32);
                break;
            }
        }

        if let (Some(graphics), Some(present)) = (graphics, present) {
            Ok(Self { graphics, present })
        } else {
            Err(SuitabilityError("Missing required queue families"))
        }
    }
}

#[derive(Debug)]
struct SuitabilityError(pub &'static str);
impl error::Error for SuitabilityError {}
impl Display for SuitabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)?;
        Ok(())
    }
}

pub fn pick_physical_device(
    entry: &Entry,
    instance: &Instance,
    surface: vk::SurfaceKHR,
) -> Result<vk::PhysicalDevice, super::Error> {
    for physical_device in unsafe { instance.enumerate_physical_devices()? } {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };

        let device_name =
            unsafe { CStr::from_ptr(properties.device_name.as_ptr()) }.to_string_lossy();

        if let Err(error) =
            unsafe { check_physical_device(entry, instance, surface, physical_device) }
        {
            warn!("Skipping physical device '{}': {}", device_name, error);
        } else {
            info!("Selected physical device '{}'.", device_name);
            return Ok(physical_device);
        }
    }

    Err(Error::new("No suitable physical device".to_string()))
}

unsafe fn check_physical_device(
    entry: &Entry,
    instance: &Instance,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
) -> Result<(), SuitabilityError> {
    QueueFamilyIndices::get(entry, instance, surface, physical_device)?;
    Ok(())
}

extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    type_: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _: *mut c_void,
) -> vk::Bool32 {
    let data = unsafe { *data };
    let message = unsafe { CStr::from_ptr(data.p_message) }.to_string_lossy();

    if severity >= vk::DebugUtilsMessageSeverityFlagsEXT::ERROR {
        error!("({:?}) {}", type_, message);
    } else if severity >= vk::DebugUtilsMessageSeverityFlagsEXT::WARNING {
        warn!("({:?}) {}", type_, message);
    } else if severity >= vk::DebugUtilsMessageSeverityFlagsEXT::INFO {
        debug!("({:?}) {}", type_, message);
    } else {
        trace!("({:?}) {}", type_, message);
    }

    vk::FALSE
}
