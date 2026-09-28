use super::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind};

impl Layout {
    /// Move an existing item without changing its identity, geometry or placement.
    /// `before` identifies a direct child of the destination; None appends.
    pub fn reparent_fixture(
        &mut self,
        id: FixtureInstanceId,
        parent: Option<FixtureInstanceId>,
        before: Option<FixtureInstanceId>,
    ) -> Result<(), String> {
        let item = self.fixture(id).ok_or("Fixture or group was not found.")?;
        fn contains(item: &LayoutFixture, id: FixtureInstanceId) -> bool {
            item.id == id
                || matches!(&item.kind, LayoutFixtureKind::Group { children } if children.iter().any(|child| contains(child, id)))
        }
        if parent.is_some_and(|parent| contains(item, parent)) {
            return Err("A group cannot be moved into itself or one of its children.".into());
        }
        let destination = match parent {
            None => &self.fixtures,
            Some(parent) => match &self
                .fixture(parent)
                .ok_or("Destination group was not found.")?
                .kind
            {
                LayoutFixtureKind::Group { children } => children,
                _ => return Err("Items can only be moved into groups.".into()),
            },
        };
        if let Some(before) = before {
            if !destination.iter().any(|item| item.id == before) {
                return Err("The insertion target is not in the destination group.".into());
            }
            if before == id {
                return Ok(());
            }
        }
        fn take(items: &mut Vec<LayoutFixture>, id: FixtureInstanceId) -> Option<LayoutFixture> {
            if let Some(index) = items.iter().position(|item| item.id == id) {
                return Some(items.remove(index));
            }
            for item in items {
                if let LayoutFixtureKind::Group { children } = &mut item.kind
                    && let Some(found) = take(children, id)
                {
                    return Some(found);
                }
            }
            None
        }
        fn children(
            items: &mut Vec<LayoutFixture>,
            parent: Option<FixtureInstanceId>,
        ) -> Option<&mut Vec<LayoutFixture>> {
            let Some(parent) = parent else {
                return Some(items);
            };
            for item in items {
                if let LayoutFixtureKind::Group { children: nested } = &mut item.kind {
                    if item.id == parent {
                        return Some(nested);
                    }
                    if let Some(found) = children(nested, Some(parent)) {
                        return Some(found);
                    }
                }
            }
            None
        }
        // Every destination check precedes removal, so rejected moves leave the tree intact.
        let item = take(&mut self.fixtures, id).ok_or("Validated source is missing.")?;
        let destination =
            children(&mut self.fixtures, parent).ok_or("Validated destination is missing.")?;
        let index = match before {
            Some(id) => destination
                .iter()
                .position(|item| item.id == id)
                .ok_or("Validated insertion target is missing.")?,
            None => destination.len(),
        };
        destination.insert(index, item);
        Ok(())
    }
}
