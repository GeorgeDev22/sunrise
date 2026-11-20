//! Core terrain types, coordinate keys, and chunk visibility/fade calculations.

/// A chunk section coordinate key `(x, y, z)`.
pub type SectionKey = (i32, i32, i32);

/// Fixed-point steps per unit of `TerrainVertex` UVs, which span `[0, 2)`.
pub const UV_STEPS: f32 = 32768.0;

/// `LevelRenderer.CHUNK_VISIBILITY_THRESHOLD`: a section's block entities wait for it.
pub const CHUNK_VISIBILITY_THRESHOLD: f32 = 0.3;

/// `SectionRenderDispatcher.RenderSection.getVisibility`: how far through its fade-in a section
/// uploaded `elapsed_ms` ago is.
pub fn chunk_visibility(elapsed_ms: u64, fade_ms: u64) -> f32 {
    if elapsed_ms >= fade_ms {
        1.0
    } else {
        elapsed_ms as f32 / fade_ms as f32
    }
}

/// `Util.toMillis`: `Mth.floor(seconds * 1000.0)`.
pub fn fade_millis(seconds: f32) -> u64 {
    (f64::from(seconds) * 1000.0).floor().max(0.0) as u64
}

/// A section's instance: its camera-relative origin and its UV step, negated
/// past its fade code while it fades in over far terrain (`lod::instance`).
pub fn section_instance(origin: [f32; 3], fade: u32, visibility: f32) -> [f32; 4] {
    let step = 1.0 / UV_STEPS;
    [
        origin[0],
        origin[1],
        origin[2],
        if fade == 0 {
            // The step is a power of two, so its mantissa is free: it holds the vanilla
            // chunk fade-in's `1 - visibility` in 23 bits (terrain.wgsl's `instance_step` and
            // `instance_visibility` read it back). A whole section is the bare step.
            let faded = ((1.0 - visibility) * 8_388_608.0)
                .round()
                .clamp(0.0, 8_388_607.0) as u32;
            f32::from_bits(step.to_bits() | faded)
        } else {
            -(fade as f32 + step)
        },
    ]
}
