//! Generic lookup: one store per kind of project object, keyed by id.
use super::*;

/// A kind of project object, stored by its id.
pub trait ProjectObject: Sized + Clone + 'static {
    type Id: crate::TypedId + Eq + std::hash::Hash + std::fmt::Debug;
    fn store(project: &DonderProject) -> &IndexMap<Self::Id, Self>;
}

/// In-place access for edits, which must detach only what they change.
pub(crate) trait Stored: ProjectObject {
    fn store_mut(project: &mut DonderProject) -> &mut IndexMap<Self::Id, Self>;
}

macro_rules! stored {
    ($($kind:ty => $id:ty, $store:ident;)*) => {$(
        impl ProjectObject for $kind {
            type Id = $id;
            fn store(project: &DonderProject) -> &IndexMap<$id, Self> {
                &project.$store
            }
        }
        impl Stored for $kind {
            fn store_mut(project: &mut DonderProject) -> &mut IndexMap<$id, Self> {
                &mut project.$store
            }
        }
    )*};
}

stored! {
    Layout => LayoutId, layouts;
    Controller => ControllerId, controllers;
    InputDevice => InputDeviceId, input_devices;
    Sequence => SequenceId, sequences;
    Console => ConsoleId, consoles;
    FixtureDefinition => FixtureDefinitionId, fixtures;
    CurveDefinition => CurveId, curves;
    GradientDefinition => GradientId, gradients;
    ScriptDefinition => ScriptId, scripts;
}
