//! Session identities. Loading assigns every object a fresh opaque id; names,
//! documents and addresses belong to project IO, never to the model.

macro_rules! object_ids {
    ($($kind:ident($id:ident)),* $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
            pub struct $id(pub u32);

            impl From<$id> for ObjectId {
                fn from(id: $id) -> Self {
                    Self::$kind(id)
                }
            }

            impl TryFrom<ObjectId> for $id {
                type Error = ObjectId;
                fn try_from(id: ObjectId) -> Result<Self, ObjectId> {
                    match id {
                        ObjectId::$kind(id) => Ok(id),
                        other => Err(other),
                    }
                }
            }
        )*

        /// Any top-level object of a project. Ids are unique across kinds.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
        pub enum ObjectId {
            $($kind($id),)*
        }

        /// What kind of object an id names.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
        pub enum ObjectKind {
            $($kind,)*
        }

        impl ObjectKind {
            /// The id of this kind with number `raw`.
            pub fn id(self, raw: u32) -> ObjectId {
                match self {
                    $(Self::$kind => ObjectId::$kind($id(raw)),)*
                }
            }
        }

        impl ObjectId {
            pub fn kind(self) -> ObjectKind {
                match self {
                    $(Self::$kind(_) => ObjectKind::$kind,)*
                }
            }

            /// The id's number, unique in its project.
            pub fn raw(self) -> u32 {
                match self {
                    $(Self::$kind($id(raw)))|* => raw,
                }
            }
        }
    };
}

object_ids! {
    Project(ProjectId),
    Setup(SetupId),
    Layout(LayoutId),
    Patch(PatchId),
    Controller(ControllerId),
    InputDevice(InputDeviceId),
    Sequence(SequenceId),
    Rig(RigId),
    FixtureDefinition(FixtureDefinitionId),
    Curve(CurveId),
    Gradient(GradientId),
    EffectDefinition(EffectDefinitionId),
    OperatorDefinition(OperatorDefinitionId),
}
