pub mod file;
pub mod members;

use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

pub use file::FileModule;
pub use members::MembersModule;
use crate::StudIpClient;

type ModuleConstructor = fn(Arc<CourseModuleData>) -> Box<dyn CourseModule>;

pub(crate) static COURSE_MODULE_REGISTRY: once_cell::sync::Lazy<Arc<Mutex<HashMap<&'static str, ModuleConstructor>>>> = once_cell::sync::Lazy::new(Default::default);
// Simply keeps track, if the default models have already been registered
pub(crate) static REGISTERED_DEFAULT_COURSE_MODULES: once_cell::sync::OnceCell<()> = once_cell::sync::OnceCell::new();


pub trait CourseModule: Debug + Any {
    /// Constructs a new instance of the Module, for a specific [Course](crate::course::Course)
    fn new(data: Arc<CourseModuleData>) -> Self where Self: Sized;

    /// The name of the course module. \
    /// Needs to correspond to the id of the tab in the HTML (without the prefix: `nav_course_`)
    fn name() -> &'static str where Self: Sized;

    /// Converts the Module to [`Any`], required for downcasting back to a concrete type
    fn as_any(&mut self) -> &mut dyn Any;
}

/// Registers a course module globally, only registered modules can be detected by [Course::query_modules()](crate::course::Course::query_modules())
pub fn register_course_module<M: CourseModule + 'static>() {
    let mut registry = COURSE_MODULE_REGISTRY.lock().unwrap();
    registry.insert(M::name(), |data| Box::new(M::new(data)));
}

/// Gets a downcasted [`CourseModule`], by its Type on a [Course](crate::course::Course)
#[macro_export]
macro_rules! get_module {
    ($course:expr, $module_type:ty) => {{
        $course.modules.iter_mut().find_map(|module| module.as_any().downcast_mut::<$module_type>())
    }};
}

/// Some data, that is required for any [`CourseModule`]
#[derive(Debug)]
pub struct CourseModuleData {
    pub course_id: String,
    pub client: Arc<StudIpClient>,
}

pub (crate) fn register_default_course_modules() {
    register_course_module::<FileModule>();
    register_course_module::<MembersModule>();
}