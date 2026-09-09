pub mod authoring;

use indexmap::IndexMap;

use crate::element::{ElementCellAddress, ElementTreeId};
use crate::identity::SourceIdentity;
use crate::values::{DistanceSpan, Point3, Rotation3, Scale3};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PreviewLayoutId(pub SourceIdentity);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PropDefinitionId(pub SourceIdentity);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PropInstanceId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewLayout {
    pub id: PreviewLayoutId,
    pub element_tree: ElementTreeId,
    pub props: Vec<PropInstance>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropDefinition {
    pub bulb_radius: DistanceSpan,
    pub geometry: PropGeometry,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropGeometry {
    Points {
        points: Vec<Point3>,
    },
    Lines {
        points: Vec<Point3>,
        point_count: u32,
    },
    Arc {
        center: Point3,
        radius: DistanceSpan,
        start_degrees: f32,
        end_degrees: f32,
        point_count: u32,
    },
}

impl PropGeometry {
    pub fn point_count(&self) -> usize {
        match self {
            Self::Points { points } => points.len(),
            Self::Lines { point_count, .. } | Self::Arc { point_count, .. } => {
                *point_count as usize
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropInstance {
    pub id: PropInstanceId,
    pub name: String,
    pub definition: PropDefinitionId,
    pub position: Point3,
    pub rotation: Rotation3,
    pub scale: Scale3,
    pub bindings: Vec<ElementCellAddress>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropDefinitionStore {
    pub definitions: IndexMap<PropDefinitionId, PropDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewValidationError {
    DuplicateProp(PropInstanceId),
    MissingDefinition(PropDefinitionId),
    BindingCount {
        prop: PropInstanceId,
        expected: usize,
        actual: usize,
    },
    MissingElementCell {
        prop: PropInstanceId,
        address: ElementCellAddress,
    },
}
