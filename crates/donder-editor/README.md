# Shared Donder editor

This crate owns typed GUI projection, mutations, selection, clipboard, and domain
conversion used by both desktop and browser hosts. Its GUI modules were extracted
from the desktop implementation; hosts must not duplicate their authoring logic.

`donder-sequence-api` owns shared DTOs. `donder-project-io` owns project/source
metadata, imports, and serialization. Hosts own transaction acceptance, history,
playback refresh, and platform IO. Edit helpers mutate the supplied candidate
`ProjectSession`; they do not make another transactional project clone.
