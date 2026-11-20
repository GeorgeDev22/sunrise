//! The published light of columns the level does not hold.
//!
//! `LayerLightSectionStorage.updateSectionStatus` keeps sections touched by a non-air section
//! (the full 3x3x3 cube). `queueLightRemoval` drops a column's own support, but keeps its
//! last stored layers where another column still supports them.

use crate::chunk::{Chunk, Light, index};
use crate::section_mask::SectionMask;
use crate::world::World;

/// Retained or newly supported layers of a missing column, indexed from its lower pad.
#[derive(Default)]
pub struct Border(Vec<Layer>);

struct Layer {
    section: usize,
    sky: bool,
    block: bool,
    values: Light,
}

fn neighbours((x, z): (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    (-1..=1).flat_map(move |dz| {
        (-1..=1).filter_map(move |dx| ((dx, dz) != (0, 0)).then_some((x + dx, z + dz)))
    })
}

impl World {
    fn border_support(&self, key: (i32, i32), except: Option<(i32, i32)>) -> SectionMask {
        neighbours(key)
            .filter(|&near| except != Some(near))
            .filter_map(|near| self.chunks.get(&near))
            .fold(SectionMask::EMPTY, |a, c| a | c.light_support)
    }

    /// `SkyLightSectionStorage.getLightValue`: an unstored section uses the bottom row of
    /// the nearest stored sky layer above it (15 if there isn't one). Block light does not inherit.
    pub(crate) fn border_light(&self, key: (i32, i32), x: i32, y: i32, z: i32) -> u8 {
        let Some(border) = self.border.get(&key) else {
            return 0xF0;
        };
        let section = i64::from(y >> 4) - i64::from(self.layout().min_y() >> 4) + 1;
        let start = border
            .0
            .partition_point(|layer| (layer.section as i64) < section);
        let layers = &border.0[start..];
        let cell = index(x, y, z);
        let block = layers.first().map_or(0, |layer| {
            if layer.block && layer.section as i64 == section {
                layer.values.get(cell) & 15
            } else {
                0
            }
        });
        let sky = layers.iter().find(|layer| layer.sky).map_or(0xF0, |layer| {
            let i = if layer.section as i64 == section {
                cell
            } else {
                cell & 255
            };
            layer.values.get(i) & 0xF0
        });
        sky | block
    }

    /// Install a column with its light section status already sampled at publication, not on reads.
    pub fn install_light_column(&mut self, chunk: Chunk) {
        let key = (chunk.x, chunk.z);
        let old = self.chunks.insert(key, std::sync::Arc::new(chunk));
        self.publish_light_border(key, old.map_or(SectionMask::EMPTY, |c| c.light_support));
    }

    /// Update the neighbouring missing columns for a new section-status snapshot.
    pub(crate) fn publish_light_border(&mut self, key: (i32, i32), before: SectionMask) {
        self.border.remove(&key);
        let current = self.chunks[&key].light_support;
        if current == before {
            return;
        }
        for near in neighbours(key).filter(|near| !self.chunks.contains_key(near)) {
            let other = self.border_support(near, Some(key));
            let added = current & !(before | other);
            let supported = current | other;
            if added.is_empty() && !self.border.contains_key(&near) {
                continue;
            }
            let sky = self.shape.sky_light;
            let border = self.border.entry(near).or_default();
            for section in added.iter() {
                let i = border.0.partition_point(|layer| layer.section < section);
                // `SkyLightSectionStorage.createDataLayer`: an unlit column starts dark, or
                // copies the bottom row of an existing sky layer above the newly stored one.
                let values = match border.0[i..]
                    .iter()
                    .find(|layer| layer.sky)
                    .map(|layer| &layer.values)
                {
                    None => Light::Uniform(0),
                    Some(Light::Uniform(value)) => Light::Uniform(value & 0xF0),
                    Some(Light::Block(_, sky)) => Light::Uniform(sky << 4),
                    Some(light) => {
                        let mut data = Box::new([0; 4096]);
                        for (i, v) in data.iter_mut().enumerate() {
                            *v = light.get(i & 255) & 0xF0;
                        }
                        Light::packed(data)
                    }
                };
                border.0.insert(
                    i,
                    Layer {
                        section,
                        sky,
                        block: true,
                        values,
                    },
                );
            }
            // Section status adds immediately; pending removals are applied when light publishes.
            border.0.retain(|layer| supported.contains(layer.section));
            if border.0.is_empty() {
                self.border.remove(&near);
            }
        }
    }

    /// The column has just left: keep only its actually stored layers a neighbour still supports.
    pub(crate) fn retain_border(&mut self, key: (i32, i32), chunk: &Chunk) {
        let supported = self.border_support(key, None);
        let kept: Vec<_> = supported
            .iter()
            .filter_map(|b| {
                let light = chunk.lit.then(|| chunk.light_section(b)).flatten()?;
                let [sky, block] = chunk
                    .stored_light_layers(b)
                    .unwrap_or([self.shape.sky_light, true]);
                (sky || block).then(|| Layer {
                    section: b,
                    sky,
                    block,
                    values: light.light.clone(),
                })
            })
            .collect();
        if kept.is_empty() {
            self.border.remove(&key);
        } else {
            self.border.insert(key, Border(kept));
        }
        for near in neighbours(key) {
            if !self.border.contains_key(&near) {
                continue;
            }
            let supported = self.border_support(near, None);
            let border = self.border.get_mut(&near).expect("border exists");
            border.0.retain(|layer| supported.contains(layer.section));
            if border.0.is_empty() {
                self.border.remove(&near);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chunk::SECTIONS, light::light_from_layers};
    use sunrise_world::registry::BlockStateId;

    #[test]
    fn a_retained_gap_uses_the_nearest_stored_sky_not_cached_inheritance() {
        let mut world = World::new(BlockStateId(0), 0);
        let mut neighbour = Chunk::empty(0, 0);
        neighbour.add_light_support(1);
        neighbour.add_light_support(5);
        world.install_light_column(neighbour);
        let mut chunk = Chunk::empty(1, 0);
        chunk.add_light_support(3);
        let mut layers = vec![(None, None); SECTIONS + 2];
        layers[2].1 = Some(vec![0x77; 2048]);
        layers[3].0 = Some(vec![0; 2048]);
        layers[4].0 = Some(vec![0xAA; 2048]); // supported only by the column being removed
        layers[6].0 = Some(vec![0xCC; 2048]);
        light_from_layers(&mut chunk, &layers, true);
        world.install_light_column(chunk);
        world.unload_chunk((1, 0)).unwrap();
        assert_eq!(world.light(16, -48, 0), 7); // block-only section borrows from the dark row
        assert_eq!(world.light(16, -16, 0), 0xC0); // removed row borrows from the higher row
        assert_eq!(world.light(16, -200, 0), 0); // no fabricated lower pad or stale packed sky
        assert_eq!(world.light(16, 48, 0), 0xF0);
    }
}
