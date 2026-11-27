# Accepted props

The props a world accepts (`vox-server --props <folder>`; this folder by default). Each world reads it once when it opens.

- `catalogue.json`: the accepted props, as `vox_format::prop::Catalogue` writes them (each entry's name, its tags and its hash).
- `<hash>.voxp`: one blob per accepted prop, named by its hash (`vox_format::prop::PropBlob`).

A missing folder or catalogue means the world accepts no props. A blob that does not decode to its catalogue name is left out and logged.

A prop already placed in a world whose hash is later taken out of the catalogue stays in that world as an orphan: it takes no cells (others may build where it stands), it cannot be moved or turned, and whoever may remove a prop may remove it. Putting its hash back in the catalogue restores it the next time the world opens.
